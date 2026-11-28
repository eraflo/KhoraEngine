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

//! The asset resolver: what the references ([`MeshRef`], [`MaterialRef`], a UI
//! font or image) name, put into the asset store under the reference's uuid.
//!
//! See the book: concepts/assets.md, "From reference to copy".

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use khora_core::asset::{Asset, AssetHandle, AssetUUID, Material};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::Runtime;
use khora_data::assets::{ArrivalKind, ArrivalsRead, Assets};
use khora_data::ecs::{
    reconstruct_procedural_mesh, Changed, ComponentKey, DataSystemRegistration, MaterialRef,
    MeshRef, SystemContext, Tick, TickPhase, World,
};
use khora_data::AssetStore;

use super::ui_asset_resolver;
use crate::asset::{AssetService, Requested};

/// The resolver's name: its `DataSystem`'s, and the reader it reads the
/// material and mesh stores' arrivals as.
const ASSET_RESOLVER: &str = "asset_resolver";

fn asset_resolver_system(
    world: &mut World,
    runtime: &Runtime,
    _deck: &mut OutputDeck,
    ctx: SystemContext,
) {
    resolve_references(world, runtime, ctx.since);
    ui_asset_resolver::resolve_ui_fonts(world, runtime);
    ui_asset_resolver::resolve_ui_images(world, runtime);
}

/// Resolves the references changed since `since` and those whose decode landed since.
///
/// Returns how many `MeshRef` and `MaterialRef` rows it read, each counting
/// one. The registered `asset_resolver` runs it every frame. Attaches nothing
/// to any entity; does nothing without an asset store.
fn resolve_references(world: &mut World, runtime: &Runtime, since: Tick) -> usize {
    let Some(store) = runtime.resources.get::<AssetStore>() else {
        return 0;
    };
    let service = runtime.services.get::<Arc<Mutex<AssetService>>>();
    let mut resolver = Resolver {
        store,
        service: service.map(|service| &**service),
        wanted: service.map_or_else(HashSet::new, |service| {
            service.lock_recovered().take_resolving()
        }),
    };
    resolver.ask_again();
    let read = resolver.materials_changed(world, since) + resolver.meshes_changed(world, since);
    resolver.follow_arrivals();
    if let Some(service) = resolver.service {
        service.lock_recovered().keep_resolving(resolver.wanted);
    }
    read
}

/// Asks for every texture `material` names that `textures` does not hold yet.
///
/// One cached is stored now; returns the others, being decoded or failed,
/// which land in the store when decoded. A texture that failed waits for a
/// file to change.
pub(super) fn load_textures(
    material: &dyn Material,
    textures: &RwLock<Assets<CpuTexture>>,
    service: &mut AssetService,
) -> Vec<AssetUUID> {
    let mut waiting = Vec::new();
    for uuid in material.textures() {
        if textures.read_recovered().contains(&uuid) {
            continue;
        }
        match service.request::<CpuTexture>(&uuid) {
            Requested::Ready(texture) => textures.write_recovered().insert(uuid, texture),
            Requested::Pending => waiting.push(uuid),
            Requested::Failed => {
                log::debug!(
                    "asset_resolver: texture {uuid:?} failed to decode; it waits for a file to change"
                );
                waiting.push(uuid);
            }
        }
    }
    waiting
}

/// One run of the resolver: the store it fills, the service it asks, and what
/// it asked for that has not landed.
struct Resolver<'a> {
    store: &'a AssetStore,
    service: Option<&'a Mutex<AssetService>>,
    /// The assets, by uuid and type, asked for and not landed: being decoded,
    /// or failed and waiting for a file to change.
    wanted: HashSet<(AssetUUID, TypeId)>,
}

impl Resolver<'_> {
    /// Asks the service again for every asset wanted, keeping those that have not landed.
    ///
    /// A failed one is not decoded again until a file changes; one whose file
    /// left the project is not asked for until it is back.
    fn ask_again(&mut self) {
        let Some(service) = self.service else {
            return;
        };
        if self.wanted.is_empty() {
            return;
        }
        let mut svc = service.lock_recovered();
        let wanted = std::mem::take(&mut self.wanted);
        for (uuid, type_id) in wanted {
            if svc.gone_from_index(&uuid) {
                self.wanted.insert((uuid, type_id));
                continue;
            }
            if type_id == TypeId::of::<Mesh>() {
                self.ask::<Mesh>(&mut svc, uuid);
            } else if type_id == TypeId::of::<Box<dyn Material>>() {
                self.ask::<Box<dyn Material>>(&mut svc, uuid);
            } else if type_id == TypeId::of::<CpuTexture>() {
                self.ask::<CpuTexture>(&mut svc, uuid);
            }
        }
    }

    /// Asks `svc` for the `A` named `uuid`: stored if ready, else wanted.
    fn ask<A: Asset>(&mut self, svc: &mut AssetService, uuid: AssetUUID) {
        match svc.request::<A>(&uuid) {
            Requested::Ready(handle) => publish(&self.store.store::<A>(), uuid, &handle),
            Requested::Pending | Requested::Failed => {
                self.wanted.insert((uuid, TypeId::of::<A>()));
            }
        }
    }

    /// Resolves every `MaterialRef` changed since `since`; returns how many it read.
    ///
    /// An inline material is stored under its uuid unless the store holds it;
    /// an asset is asked of the service.
    fn materials_changed(&mut self, world: &World, since: Tick) -> usize {
        let store = self.store.store::<Box<dyn Material>>();
        let mut read = 0;
        let mut inline: Vec<(AssetUUID, Box<dyn Material>)> = Vec::new();
        let mut assets: Vec<(EntityId, AssetUUID)> = Vec::new();
        {
            let held = store.read_recovered();
            let changed = world
                .query::<(EntityId, &MaterialRef, Changed<MaterialRef>)>()
                .since(since);
            for (entity, material_ref, _) in changed {
                read += 1;
                match material_ref {
                    MaterialRef::Inline { material, uuid } => {
                        if !held.contains(uuid) {
                            inline.push((*uuid, material.clone_box()));
                        }
                    }
                    MaterialRef::Asset(uuid) => assets.push((entity, *uuid)),
                }
            }
        }
        for (uuid, material) in inline {
            let mut held = store.write_recovered();
            if !held.contains(&uuid) {
                held.insert(uuid, AssetHandle::new(material));
            }
        }
        self.ask_for_assets::<Box<dyn Material>>(&assets, "material");
        read
    }

    /// Resolves every `MeshRef` changed since `since`; returns how many it read.
    ///
    /// A procedural mesh is built and stored under its uuid unless the store
    /// holds it; an asset is asked of the service. A mesh made in code the
    /// store does not hold draws nothing, and is logged.
    fn meshes_changed(&mut self, world: &World, since: Tick) -> usize {
        let store = self.store.store::<Mesh>();
        let mut read = 0;
        let mut procedural = Vec::new();
        let mut assets: Vec<(EntityId, AssetUUID)> = Vec::new();
        {
            let held = store.read_recovered();
            let changed = world
                .query::<(EntityId, &MeshRef, Changed<MeshRef>)>()
                .since(since);
            for (entity, mesh_ref, _) in changed {
                read += 1;
                match mesh_ref {
                    MeshRef::Procedural { kind, params, uuid } => {
                        if !held.contains(uuid) {
                            procedural.push((*uuid, *kind, *params));
                        }
                    }
                    MeshRef::Asset(uuid) => assets.push((entity, *uuid)),
                    MeshRef::Made(uuid) => {
                        if !held.contains(uuid) {
                            log::warn!(
                                "asset_resolver: entity {entity:?} names a mesh made in code \
                                 ({uuid:?}) the asset store does not hold; it draws nothing"
                            );
                        }
                    }
                }
            }
        }
        for (uuid, kind, params) in procedural {
            let mut held = store.write_recovered();
            if !held.contains(&uuid) {
                held.insert(
                    uuid,
                    AssetHandle::new(reconstruct_procedural_mesh(kind, params)),
                );
            }
        }
        self.ask_for_assets::<Mesh>(&assets, "mesh");
        read
    }

    /// Asks the service for each asset of `assets`, named by the entity beside it.
    fn ask_for_assets<A: Asset>(&mut self, assets: &[(EntityId, AssetUUID)], what: &str) {
        if assets.is_empty() {
            return;
        }
        let Some(service) = self.service else {
            for (entity, uuid) in assets {
                log::error!(
                    "asset_resolver: AssetService missing; cannot resolve {what} asset \
                     {uuid:?} on entity {entity:?}."
                );
            }
            return;
        };
        let mut svc = service.lock_recovered();
        for (_, uuid) in assets {
            self.ask::<A>(&mut svc, *uuid);
        }
    }

    /// Follows what arrived in the material and mesh stores since the last run.
    ///
    /// A material that landed or was replaced has its textures asked for; a
    /// material or mesh gone from the store is wanted again, for when its file
    /// is back. After lost arrivals, every stored material's textures are asked for.
    fn follow_arrivals(&mut self) {
        let materials = self.store.store::<Box<dyn Material>>();
        let read = materials.read_recovered().arrivals(ASSET_RESOLVER);
        let arrived: Vec<AssetHandle<Box<dyn Material>>> = {
            let held = materials.read_recovered();
            match read {
                ArrivalsRead::Since(arrivals) => {
                    let mut arrived = Vec::new();
                    for arrival in arrivals {
                        match arrival.kind {
                            ArrivalKind::Landed | ArrivalKind::Replaced => {
                                arrived.extend(held.get(&arrival.uuid).cloned());
                            }
                            ArrivalKind::Gone => {
                                self.wanted
                                    .insert((arrival.uuid, TypeId::of::<Box<dyn Material>>()));
                            }
                        }
                    }
                    arrived
                }
                ArrivalsRead::Resync => held
                    .keys()
                    .filter_map(|uuid| held.get(&uuid).cloned())
                    .collect(),
            }
        };
        let read = self
            .store
            .store::<Mesh>()
            .read_recovered()
            .arrivals(ASSET_RESOLVER);
        if let ArrivalsRead::Since(arrivals) = read {
            self.wanted.extend(
                arrivals
                    .iter()
                    .filter(|arrival| arrival.kind == ArrivalKind::Gone)
                    .map(|arrival| (arrival.uuid, TypeId::of::<Mesh>())),
            );
        }
        let Some(service) = self.service else {
            return;
        };
        if arrived.is_empty() {
            return;
        }
        let textures = self.store.store::<CpuTexture>();
        let mut svc = service.lock_recovered();
        for material in arrived {
            let waiting = load_textures(&**material, &textures, &mut svc);
            self.wanted.extend(
                waiting
                    .into_iter()
                    .map(|uuid| (uuid, TypeId::of::<CpuTexture>())),
            );
        }
    }
}

/// Stores `handle` as the copy of `uuid` in `store`, unless it is the one held.
fn publish<A: Asset>(store: &RwLock<Assets<A>>, uuid: AssetUUID, handle: &AssetHandle<A>) {
    let mut held = store.write_recovered();
    if held.get(&uuid) != Some(handle) {
        held.insert(uuid, handle.clone());
    }
}

inventory::submit! {
    DataSystemRegistration {
        name: ASSET_RESOLVER,
        phase: TickPhase::PreExtract,
        run: asset_resolver_system,
        // Lower than the residencies `gpu_mesh_sync` and `gpu_material_sync`
        // (order_hint 0), which also run after it, so the copies are stored
        // before they are made resident.
        order_hint: -5,
        // A changed asset's copies are replaced before the references are read.
        runs_after: &["asset_reload"],
        observes: &[ComponentKey::of::<MeshRef>, ComponentKey::of::<MaterialRef>],
        role: khora_data::ecs::Role::Upkeep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use khora_core::asset::StandardMaterial;
    use khora_core::math::LinearRgba;
    use khora_data::assets::{Arrival, ArrivalKind, ArrivalsRead};
    use khora_data::ecs::{HandleComponent, Observer};

    /// The arrivals reader these tests read the store as.
    const WATCHER: &str = "asset_resolver_tests";

    /// Starts reading the arrivals of `Assets<A>`: [`arrived`] returns what lands after this call.
    fn watch<A: Asset>(store: &AssetStore) {
        let _ = store.store::<A>().read().unwrap().arrivals(WATCHER);
    }

    /// The arrivals of `Assets<A>` since the last [`watch`] or call.
    fn arrived<A: Asset>(store: &AssetStore) -> Vec<Arrival> {
        match store.store::<A>().read().unwrap().arrivals(WATCHER) {
            ArrivalsRead::Since(arrivals) => arrivals,
            ArrivalsRead::Resync => panic!("the store is watched first"),
        }
    }

    /// The copy `store` holds under `uuid`, if any.
    fn stored<A: Asset>(store: &AssetStore, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
        store.store::<A>().read().unwrap().get(uuid).cloned()
    }

    /// The uuid `entity`'s `MaterialRef` names.
    fn material_uuid(world: &World, entity: EntityId) -> AssetUUID {
        world
            .get::<MaterialRef>(entity)
            .expect("the entity has a MaterialRef")
            .uuid()
    }

    /// The uuid `entity`'s `MeshRef` names.
    fn mesh_uuid(world: &World, entity: EntityId) -> AssetUUID {
        world
            .get::<MeshRef>(entity)
            .expect("the entity has a MeshRef")
            .uuid()
    }

    /// The `StandardMaterial` `store` holds under `uuid`.
    fn stored_standard(store: &AssetStore, uuid: &AssetUUID) -> StandardMaterial {
        let material = stored::<Box<dyn Material>>(store, uuid).expect("the material is stored");
        let material: &dyn Material = &**material;
        material
            .as_any()
            .downcast_ref::<StandardMaterial>()
            .expect("a StandardMaterial")
            .clone()
    }

    /// Runs the registered data system `name` once, as the scheduler's runner does.
    ///
    /// It is handed the tick of its last run; its writes carry this one.
    fn run_system(world: &mut World, runtime: &Runtime, name: &'static str) {
        let observer = Observer::DataSystem(name);
        let ctx = SystemContext {
            since: world.observer_last_seen(observer),
            resumed: world.observer_resumes(observer),
            scope: None,
        };
        let tick = world.begin_run();
        let system = inventory::iter::<DataSystemRegistration>
            .into_iter()
            .find(|system| system.name == name)
            .unwrap_or_else(|| panic!("`{name}` is registered"));
        (system.run)(world, runtime, &mut OutputDeck::default(), ctx);
        world.note_observer_run(observer, tick);
        world.end_run();
    }

    /// Runs the resolver once, as the scheduler's runner does.
    fn resolve_once(world: &mut World, runtime: &Runtime) {
        run_system(world, runtime, ASSET_RESOLVER);
    }

    /// Runs frames of the resolver until what it asked for is decoded and taken in.
    fn resolve_settled(world: &mut World, runtime: &Runtime) {
        crate::systems::asset_intake::tests::settle(runtime, world, |world| {
            resolve_once(world, runtime);
        });
    }

    /// A runtime holding `store`.
    fn runtime_with(store: &AssetStore) -> Runtime {
        let mut runtime = Runtime::default();
        runtime.resources.insert(store.clone());
        runtime
    }

    #[test]
    fn resolves_inline_material_into_the_store_under_its_uuid() {
        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let distinctive = StandardMaterial {
            base_color: LinearRgba::new(0.2, 0.4, 0.6, 1.0),
            roughness: 0.33,
            metallic: 0.7,
            ..Default::default()
        };
        let entity = world.spawn(MaterialRef::inline(Box::new(distinctive.clone())));

        resolve_once(&mut world, &runtime);

        let standard = stored_standard(&store, &material_uuid(&world, entity));
        assert_eq!(standard.base_color, distinctive.base_color);
        assert_eq!(standard.roughness, distinctive.roughness);
        assert_eq!(standard.metallic, distinctive.metallic);
    }

    #[test]
    fn inline_resolution_is_idempotent() {
        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);
        let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial::default())));

        resolve_once(&mut world, &runtime);
        let uuid = material_uuid(&world, entity);
        let first = stored::<Box<dyn Material>>(&store, &uuid).expect("first pass resolves");

        // A second pass finds the copy already stored and changes nothing.
        resolve_once(&mut world, &runtime);
        let second = stored::<Box<dyn Material>>(&store, &uuid).expect("the copy stays");
        assert!(first == second, "the same copy is kept");
    }

    /// A `MaterialRef` changed to another inline material resolves under its new uuid.
    ///
    /// The new copy lands in the store under that uuid: the arrival the
    /// material residency makes its GPU copy on.
    #[test]
    fn changing_inline_material_stores_the_new_copy_under_its_uuid() {
        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let mat_a = StandardMaterial {
            base_color: LinearRgba::new(0.1, 0.2, 0.3, 1.0),
            ..Default::default()
        };
        let entity = world.spawn(MaterialRef::inline(Box::new(mat_a)));
        resolve_once(&mut world, &runtime);
        let uuid_a = material_uuid(&world, entity);
        assert!(
            stored::<Box<dyn Material>>(&store, &uuid_a).is_some(),
            "A resolves"
        );
        watch::<Box<dyn Material>>(&store);

        // The developer/editor swaps in a different inline material.
        let mat_b = StandardMaterial {
            base_color: LinearRgba::new(0.9, 0.8, 0.7, 1.0),
            roughness: 0.5,
            ..Default::default()
        };
        assert!(
            world.set_component(entity, MaterialRef::inline(Box::new(mat_b))),
            "replacing the MaterialRef must succeed"
        );
        resolve_once(&mut world, &runtime);

        let uuid_b = material_uuid(&world, entity);
        assert_ne!(
            uuid_a, uuid_b,
            "different material content => different uuid"
        );
        assert_eq!(
            arrived::<Box<dyn Material>>(&store),
            vec![Arrival {
                uuid: uuid_b,
                kind: ArrivalKind::Landed,
            }],
            "the new copy lands under its uuid, for the residency to make it resident"
        );
    }

    /// Re-running the resolver with no ref change stores nothing again: the
    /// store records no arrival, so the residency keeps its GPU copy.
    #[test]
    fn steady_state_material_resolution_stores_nothing_again() {
        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial::default())));
        resolve_once(&mut world, &runtime);
        let uuid = material_uuid(&world, entity);
        assert!(
            stored::<Box<dyn Material>>(&store, &uuid).is_some(),
            "resolves"
        );
        watch::<Box<dyn Material>>(&store);

        resolve_once(&mut world, &runtime);

        assert!(
            stored::<Box<dyn Material>>(&store, &uuid).is_some(),
            "steady state keeps the copy"
        );
        assert!(
            arrived::<Box<dyn Material>>(&store).is_empty(),
            "steady state stores nothing again"
        );
    }

    /// A second entity naming an inline material the store holds shares that
    /// copy: nothing is stored again, so no GPU copy is made twice.
    #[test]
    fn an_entity_naming_a_stored_inline_material_shares_its_copy() {
        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);
        let material = || MaterialRef::inline(Box::new(StandardMaterial::default()));

        let first = world.spawn(material());
        resolve_once(&mut world, &runtime);
        watch::<Box<dyn Material>>(&store);
        let second = world.spawn(material());
        resolve_once(&mut world, &runtime);

        assert_eq!(
            material_uuid(&world, first),
            material_uuid(&world, second),
            "both entities name the store's one copy"
        );
        assert_eq!(store.store::<Box<dyn Material>>().read().unwrap().len(), 1);
        assert!(
            arrived::<Box<dyn Material>>(&store).is_empty(),
            "the copy held is not stored again"
        );
    }

    /// A `MeshRef` changed to other procedural params resolves under its new uuid.
    ///
    /// The new copy lands in the store under that uuid: the arrival the mesh
    /// residency makes its GPU copy on.
    #[test]
    fn changing_procedural_mesh_stores_the_new_copy_under_its_uuid() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let entity = world.spawn(MeshRef::procedural(
            ProceduralMeshKind::Cube,
            [1.0, 0.0, 0.0, 0.0],
        ));
        resolve_once(&mut world, &runtime);
        let uuid_a = mesh_uuid(&world, entity);
        assert!(stored::<Mesh>(&store, &uuid_a).is_some(), "mesh A resolves");
        watch::<Mesh>(&store);

        // Resize the cube — different params => different content uuid.
        assert!(
            world.set_component(
                entity,
                MeshRef::procedural(ProceduralMeshKind::Cube, [4.0, 0.0, 0.0, 0.0]),
            ),
            "replacing the MeshRef must succeed"
        );
        resolve_once(&mut world, &runtime);

        let uuid_b = mesh_uuid(&world, entity);
        assert_ne!(uuid_a, uuid_b, "different params => different uuid");
        assert_eq!(
            arrived::<Mesh>(&store),
            vec![Arrival {
                uuid: uuid_b,
                kind: ArrivalKind::Landed,
            }],
            "the new copy lands under its uuid, for the residency to make it resident"
        );
    }

    #[test]
    fn resolves_asset_material_from_kmat_via_service() {
        use crate::asset::{FileLoader, IndexBuilder};
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use tempfile::tempdir;

        // A project assets dir with one `.kmat`. The on-disk form is RON of the
        // `{ type_name, material }` JSON-value tree the decoder consumes.
        let dir = tempdir().unwrap();
        let assets_root = dir.path();
        fs::create_dir_all(assets_root.join("materials")).unwrap();
        let rel_path = "materials/brass.kmat";
        let kmat = r#"{
            "type_name": "StandardMaterial",
            "material": {
                "base_color": { "r": 0.71, "g": 0.65, "b": 0.26, "a": 1.0 },
                "base_color_texture": (),
                "metallic": 1.0,
                "roughness": 0.18,
                "metallic_roughness_texture": (),
                "normal_map": (),
                "occlusion_map": (),
                "emissive": { "r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0 },
                "emissive_texture": (),
                "alpha_mode": "Opaque",
                "alpha_cutoff": 0.5,
                "double_sided": false,
            },
        }"#;
        fs::write(assets_root.join(rel_path), kmat).unwrap();

        // Build the VFS index. `.kmat` UUIDs are keyed by `new_v5` over the
        // forward-slash relative path (the IndexBuilder convention), so an
        // editor minting `MaterialRef::Asset` uses the same key.
        let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            assets_root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let uuid = AssetUUID::new_v5(rel_path);

        let metrics = Arc::new(MetricsRegistry::new());
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(assets_root)),
            metrics,
            None,
        )
        .unwrap();
        // Pulls in the inventory-registered material decoder.
        service.register_inventory_decoders();

        let store = AssetStore::new();
        let mut runtime = runtime_with(&store);
        runtime.resources.insert(khora_core::lane::Reserve::new());
        runtime
            .services
            .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

        let mut world = World::new();
        world.spawn(MaterialRef::Asset(uuid));

        // Decoded off the frame: resolved once its decode was taken in.
        resolve_settled(&mut world, &runtime);

        // The store holds the decoded material under the asset's stable uuid,
        // so the GPU projection (and a second referencing entity) see it.
        let standard = stored_standard(&store, &uuid);
        assert_eq!(standard.metallic, 1.0);
        assert_eq!(standard.roughness, 0.18);
        assert_eq!(standard.base_color.r, 0.71);
    }

    #[test]
    fn resolves_procedural_mesh_into_the_store_with_cube_counts() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let entity = world.spawn(MeshRef::procedural(
            ProceduralMeshKind::Cube,
            [2.0, 0.0, 0.0, 0.0],
        ));

        resolve_once(&mut world, &runtime);

        let mesh = stored::<Mesh>(&store, &mesh_uuid(&world, entity))
            .expect("the procedural mesh is stored under its content uuid");
        assert_eq!(mesh.positions.len(), 24, "cube has 24 vertices");
        assert_eq!(
            mesh.indices.as_ref().map_or(0, |i| i.len()),
            36,
            "cube has 36 indices"
        );
    }

    /// Two entities with identical procedural meshes dedup to one content uuid.
    #[test]
    fn identical_procedural_meshes_share_one_uuid() {
        use khora_data::ecs::ProceduralMeshKind;

        let mut world = World::new();
        let store = AssetStore::new();
        let runtime = runtime_with(&store);

        let make = || MeshRef::procedural(ProceduralMeshKind::Sphere, [0.5, 16.0, 16.0, 0.0]);
        let a = world.spawn(make());
        let b = world.spawn(make());

        resolve_once(&mut world, &runtime);

        assert_eq!(
            mesh_uuid(&world, a),
            mesh_uuid(&world, b),
            "identical procedural meshes must share a content uuid"
        );
        assert_eq!(
            store.store::<Mesh>().read().unwrap().len(),
            1,
            "one copy is stored for both"
        );
    }

    /// End-to-end: a `MeshRef::Asset` referencing a tiny `.obj` in the VFS is
    /// loaded by the resolver through a real `AssetService`, never an empty placeholder.
    #[test]
    fn resolves_asset_mesh_from_obj_via_service() {
        use crate::asset::{FileLoader, IndexBuilder, MeshDispatcher};
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let assets_root = dir.path();
        fs::create_dir_all(assets_root.join("meshes")).unwrap();
        let rel_path = "meshes/tri.obj";
        // A single triangle — the smallest renderable OBJ.
        let obj = "v 0.0 0.0 0.0\nv 1.0 0.0 0.0\nv 0.0 1.0 0.0\nf 1 2 3\n";
        fs::write(assets_root.join(rel_path), obj).unwrap();

        let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
            assets_root,
            crate::trust::Trust::Project,
            crate::trust::Refusals::detached(),
        ))
        .build_index_bytes()
        .unwrap();
        let uuid = AssetUUID::new_v5(rel_path);

        let metrics = Arc::new(MetricsRegistry::new());
        let mut service = AssetService::new(
            &index_bytes,
            Box::new(FileLoader::new(assets_root)),
            metrics,
            None,
        )
        .unwrap();
        // The mesh dispatcher has competing impls (gltf vs obj) so it is wired
        // explicitly at the call site rather than via inventory.
        service.register_decoder::<Mesh>(
            khora_core::asset::kinds::MESH.name(),
            MeshDispatcher::default(),
        );

        let store = AssetStore::new();
        let mut runtime = runtime_with(&store);
        runtime.resources.insert(khora_core::lane::Reserve::new());
        runtime
            .services
            .insert(Arc::new(Mutex::new(service)) as Arc<Mutex<AssetService>>);

        let mut world = World::new();
        world.spawn(MeshRef::Asset(uuid));

        // Decoded off the frame: resolved once its decode was taken in.
        resolve_settled(&mut world, &runtime);

        let mesh =
            stored::<Mesh>(&store, &uuid).expect("the asset mesh is stored under its stable uuid");
        assert_eq!(mesh.positions.len(), 3, "the triangle has 3 vertices");
    }

    /// Material textures: the resolver loads every texture a resolved material
    /// lists into the shared `Assets<CpuTexture>`.
    mod material_textures {
        use super::*;
        use crate::asset::{FileLoader, IndexBuilder};
        use khora_core::renderer::api::resource::CpuTexture;
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use std::path::Path;
        use tempfile::TempDir;

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
            let entity = world.spawn(MaterialRef::Asset(AssetUUID::new_v5(kmat_path)));

            project.tick(&mut world);

            assert!(
                stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity))
                    .is_some(),
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
            world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(tex),
                ..Default::default()
            })));

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
            world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(base),
                metallic_roughness_texture: Some(mr),
                normal_map: Some(normal),
                emissive_texture: Some(emissive),
                occlusion_map: Some(ao),
                ..Default::default()
            })));

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
            world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(kept),
                normal_map: Some(fresh),
                ..Default::default()
            })));

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
            let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(missing),
                metallic_roughness_texture: Some(corrupt),
                normal_map: Some(good),
                ..Default::default()
            })));

            project.tick(&mut world);

            assert!(
                project.texture(&good).is_some(),
                "the slot that loads is in Assets<CpuTexture> despite its siblings failing"
            );
            assert!(project.texture(&missing).is_none());
            assert!(project.texture(&corrupt).is_none());
            assert!(
                stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity))
                    .is_some(),
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
            let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(tex),
                ..Default::default()
            })));

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
                MaterialRef::inline(Box::new(StandardMaterial {
                    base_color_texture: Some(tex),
                    roughness: 0.9,
                    ..Default::default()
                })),
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
            let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(base),
                normal_map: Some(normal),
                ..Default::default()
            })));

            project.tick(&mut world);

            let resolved =
                stored::<Box<dyn Material>>(&project.store, &material_uuid(&world, entity))
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
            let entity = world.spawn(MaterialRef::Asset(material));

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
            let entity = world.spawn(MaterialRef::inline(Box::new(StandardMaterial {
                base_color_texture: Some(tex),
                ..Default::default()
            })));
            project.tick(&mut world);
            let before = project.texture(&tex).expect("the first image loads");
            assert_eq!((before.size.width, before.size.height), (2, 2));

            // Hot reload: the file changes and its cache entry is dropped.
            write_file(project.dir.path(), tex_path, &png(5, 5));
            assert!(project.service.lock().unwrap().invalidate(&tex));

            // The material is edited: a new ref, resolved again with its texture.
            assert!(world.set_component(
                entity,
                MaterialRef::inline(Box::new(StandardMaterial {
                    base_color_texture: Some(tex),
                    roughness: 0.9,
                    ..Default::default()
                })),
            ));
            project.tick(&mut world);

            let after = project.texture(&tex).expect("the texture is in the store");
            assert_eq!(
                (after.size.width, after.size.height),
                (5, 5),
                "the store serves the image decoded from the file's current bytes"
            );
        }
    }

    /// UI assets: the resolver gives each `UiText` the font it names and
    /// loads every `UiImage` texture into the shared `Assets<CpuTexture>`.
    mod ui_assets {
        use super::*;
        use crate::asset::{FileLoader, IndexBuilder};
        use khora_core::asset::font::Font;
        use khora_core::math::Vec2;
        use khora_core::renderer::api::resource::CpuTexture;
        use khora_data::ui::components::{UiImage, UiText, UiTransform};
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use std::path::Path;
        use tempfile::TempDir;

        /// A project assets dir on disk, served by a real `AssetService` in a runtime.
        ///
        /// Its decoders are the registered ones; the runtime holds a shared
        /// `AssetStore`.
        struct Project {
            _dir: TempDir,
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
                    _dir: dir,
                    service,
                    store,
                    runtime,
                }
            }

            /// Frames of the resolver until what it asked for is decoded off
            /// the frame and taken in at a boundary.
            fn resolve(&self, world: &mut World) {
                resolve_settled(world, &self.runtime);
            }

            fn font(&self, uuid: &AssetUUID) -> Option<AssetHandle<Font>> {
                self.store
                    .store::<Font>()
                    .read()
                    .unwrap()
                    .get(uuid)
                    .cloned()
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
            let img = image::RgbaImage::from_pixel(width, height, image::Rgba([10, 20, 30, 255]));
            image::DynamicImage::ImageRgba8(img)
                .write_to(
                    &mut std::io::Cursor::new(&mut bytes),
                    image::ImageFormat::Png,
                )
                .expect("encode png");
            bytes
        }

        fn transform() -> UiTransform {
            UiTransform {
                pos: Vec2::new(0.0, 0.0),
                size: Vec2::new(100.0, 20.0),
                z_index: 0,
            }
        }

        fn text(font: AssetUUID) -> UiText {
            UiText {
                content: "Hello".to_owned(),
                font,
                ..Default::default()
            }
        }

        fn font_handle(uuid: AssetUUID, data: &[u8]) -> HandleComponent<Font> {
            HandleComponent {
                handle: AssetHandle::new(Font {
                    name: "Stale".to_owned(),
                    data: data.to_vec(),
                }),
                uuid,
            }
        }

        /// The bytes of a font file; the font decoder keeps them as they are.
        // A TrueType file starts with these bytes: the font decoder reads only
        // what does.
        const FONT_BYTES: &[u8] = b"\0\x01\0\0font file bytes";

        #[test]
        fn a_text_naming_a_font_asset_gets_its_handle_and_the_font_lands_in_the_store() {
            let path = "fonts/body.ttf";
            let font = AssetUUID::new_v5(path);
            let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
            let mut world = World::new();
            let entity = world.spawn((transform(), text(font)));

            project.resolve(&mut world);

            let resolved = world
                .get::<HandleComponent<Font>>(entity)
                .expect("the text's font is resolved onto the entity");
            assert_eq!(resolved.uuid, font);
            assert_eq!(resolved.handle.data, FONT_BYTES);
            let stored = project
                .font(&font)
                .expect("the font is loaded into Assets<Font>");
            assert_eq!(stored.data, FONT_BYTES);
        }

        #[test]
        fn a_font_handle_naming_another_font_is_replaced_by_the_named_one() {
            let path = "fonts/title.ttf";
            let font = AssetUUID::new_v5(path);
            let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
            let mut world = World::new();
            let entity = world.spawn((
                transform(),
                text(font),
                font_handle(AssetUUID::new(), b"former font"),
            ));

            project.resolve(&mut world);

            let resolved = world
                .get::<HandleComponent<Font>>(entity)
                .expect("the entity keeps a font handle");
            assert_eq!(resolved.uuid, font, "the handle follows the text's font");
            assert_eq!(resolved.handle.data, FONT_BYTES);
        }

        #[test]
        fn a_resolved_font_is_not_loaded_again() {
            let path = "fonts/body.ttf";
            let font = AssetUUID::new_v5(path);
            let project = Project::new(&[(path, FONT_BYTES.to_vec())]);
            let mut world = World::new();
            world.spawn((transform(), text(font)));

            project.resolve(&mut world);
            let loads = project.load_count();
            project.resolve(&mut world);

            assert_eq!(loads, 1, "the font is loaded once");
            assert_eq!(project.load_count(), loads, "steady state loads nothing");
        }

        #[test]
        fn a_built_in_font_loads_nothing_and_drops_a_stale_handle() {
            let project = Project::new(&[]);
            let mut world = World::new();
            let entity = world.spawn((
                transform(),
                text(Font::BUILT_IN),
                font_handle(AssetUUID::new(), b"former font"),
            ));

            project.resolve(&mut world);

            assert_eq!(project.load_count(), 0, "the built-in face is no asset");
            assert!(
                world.get::<HandleComponent<Font>>(entity).is_none(),
                "a text in the built-in font holds no font handle"
            );
        }

        #[test]
        fn a_ui_image_texture_is_loaded_into_the_store() {
            let path = "ui/icon.png";
            let texture = AssetUUID::new_v5(path);
            let project = Project::new(&[(path, png(3, 2))]);
            let mut world = World::new();
            world.spawn((transform(), UiImage { texture }));

            project.resolve(&mut world);

            let stored = project
                .texture(&texture)
                .expect("the image is loaded into Assets<CpuTexture>");
            assert_eq!((stored.size.width, stored.size.height), (3, 2));
        }

        #[test]
        fn a_ui_image_texture_already_in_the_store_is_not_reloaded() {
            let path = "ui/icon.png";
            let texture = AssetUUID::new_v5(path);
            let project = Project::new(&[(path, png(3, 2))]);
            let mut world = World::new();
            world.spawn((transform(), UiImage { texture }));

            project.resolve(&mut world);
            let loads = project.load_count();
            project.resolve(&mut world);

            assert_eq!(loads, 1, "the image is loaded once");
            assert_eq!(project.load_count(), loads, "steady state loads nothing");
        }
    }

    /// The resolver reads the references that changed since its last run and
    /// the decodes that landed since; it puts its copies in the asset store only.
    mod watched_references {
        use super::*;
        use crate::asset::{FileLoader, IndexBuilder};
        use khora_core::math::Vec3;
        use khora_data::ecs::{Observer, ProceduralMeshKind, Transform};
        use khora_telemetry::MetricsRegistry;
        use std::fs;
        use std::path::Path;
        use tempfile::TempDir;

        const RESOLVER: &str = "asset_resolver";

        /// A project assets dir on disk, served by a real `AssetService` in a runtime.
        ///
        /// The service has the registered decoders; the runtime holds a shared
        /// `AssetStore` and the reserve the `loading` family stages into.
        struct Project {
            dir: TempDir,
            service: Arc<Mutex<AssetService>>,
            store: AssetStore,
            runtime: Runtime,
        }

        impl Project {
            fn new(files: &[(&str, Vec<u8>)]) -> Self {
                let dir = tempfile::tempdir().expect("a temporary project");
                for (rel, bytes) in files {
                    write_file(dir.path(), rel, bytes);
                }
                let index_bytes = IndexBuilder::new(crate::trust::FileGuard::new(
                    dir.path(),
                    crate::trust::Trust::Project,
                    crate::trust::Refusals::detached(),
                ))
                .build_index_bytes()
                .expect("the project indexes");
                let mut service = AssetService::new(
                    &index_bytes,
                    Box::new(FileLoader::new(dir.path())),
                    Arc::new(MetricsRegistry::new()),
                    None,
                )
                .expect("an asset service over the project");
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

            /// Runs the registered data system `name` once, as the scheduler's runner does.
            ///
            /// It is handed the tick of its last run; its writes carry this one.
            fn run_system(&self, world: &mut World, name: &'static str) {
                let observer = Observer::DataSystem(name);
                let ctx = khora_data::ecs::SystemContext {
                    since: world.observer_last_seen(observer),
                    resumed: world.observer_resumes(observer),
                    scope: None,
                };
                let tick = world.begin_run();
                let system = inventory::iter::<DataSystemRegistration>
                    .into_iter()
                    .find(|system| system.name == name)
                    .unwrap_or_else(|| panic!("`{name}` is registered"));
                (system.run)(world, &self.runtime, &mut OutputDeck::default(), ctx);
                world.note_observer_run(observer, tick);
                world.end_run();
            }

            /// One frame of the asset systems, whose resolver run reports the reference rows it read.
            ///
            /// The boundary's intake, then the registered reload and the
            /// resolver. Nothing is decoded during it: what it files waits for
            /// [`Self::decode`].
            fn counted_frame(&self, world: &mut World) -> usize {
                crate::systems::asset_intake::take_in_decoded(&self.runtime);
                self.run_system(world, "asset_reload");
                let observer = Observer::DataSystem(RESOLVER);
                let since = world.observer_last_seen(observer);
                let tick = world.begin_run();
                let visited = resolve_references(world, &self.runtime, since);
                world.note_observer_run(observer, tick);
                world.end_run();
                visited
            }

            /// Frames until nothing more is filed, each followed by the `loading` family's run.
            fn settle(&self, world: &mut World) {
                crate::systems::asset_intake::tests::settle(&self.runtime, world, |world| {
                    self.run_system(world, "asset_reload");
                    self.run_system(world, RESOLVER);
                });
            }

            /// One run of the `loading` family between two frames; returns how
            /// many decodes it took.
            fn decode(&self) -> usize {
                crate::systems::asset_intake::tests::decode_filed(&self.runtime)
            }

            fn write(&self, rel: &str, bytes: &[u8]) {
                write_file(self.dir.path(), rel, bytes);
            }

            /// Announces the asset `uuid` changed, as the editor's hot reload does.
            fn invalidate(&self, uuid: &AssetUUID) {
                self.service
                    .lock()
                    .expect("the service's lock")
                    .invalidate(uuid);
            }

            fn stored<A: Asset>(&self, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
                self.store
                    .store::<A>()
                    .read()
                    .expect("the store's lock")
                    .get(uuid)
                    .cloned()
            }

            /// The roughness of the `StandardMaterial` the store holds under `uuid`.
            fn stored_roughness(&self, uuid: &AssetUUID) -> Option<f32> {
                self.stored::<Box<dyn Material>>(uuid).map(|material| {
                    let material: &dyn Material = &**material;
                    material
                        .as_any()
                        .downcast_ref::<StandardMaterial>()
                        .expect("a StandardMaterial")
                        .roughness
                })
            }
        }

        fn write_file(root: &Path, rel: &str, bytes: &[u8]) {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().expect("a parent folder")).expect("the folders");
            fs::write(path, bytes).expect("the file is written");
        }

        /// A real PNG of `width` x `height`, encoded in memory.
        fn png(width: u32, height: u32) -> Vec<u8> {
            let mut bytes = Vec::new();
            let img = image::RgbaImage::from_pixel(width, height, image::Rgba([90, 60, 30, 255]));
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

        fn cube(size: f32) -> MeshRef {
            MeshRef::procedural(ProceduralMeshKind::Cube, [size, 0.0, 0.0, 0.0])
        }

        fn tinted(red: f32) -> MaterialRef {
            MaterialRef::inline(Box::new(StandardMaterial {
                base_color: LinearRgba::new(red, 0.5, 0.5, 1.0),
                ..Default::default()
            }))
        }

        /// The resolver watches both references, so a write to either is a change it reads.
        #[test]
        fn the_resolver_observes_mesh_and_material_references() {
            let resolver = inventory::iter::<DataSystemRegistration>
                .into_iter()
                .find(|system| system.name == RESOLVER)
                .expect("the resolver is registered");
            let observed: Vec<ComponentKey> = resolver.observes.iter().map(|key| key()).collect();
            for (key, name) in [
                (ComponentKey::of::<MeshRef>(), "MeshRef"),
                (ComponentKey::of::<MaterialRef>(), "MaterialRef"),
            ] {
                assert!(observed.contains(&key), "observes {name}: {observed:?}");
            }
        }

        /// A `MeshRef` and a `MaterialRef` given other values are resolved on the next run.
        ///
        /// Their new copies are in the store, and only their two rows are read.
        #[test]
        fn a_changed_reference_is_resolved_on_the_next_run() {
            let project = Project::new(&[]);
            let mut world = World::new();
            let entity = world.spawn((cube(1.0), tinted(0.1)));
            project.counted_frame(&mut world);
            project.counted_frame(&mut world);
            watch::<Mesh>(&project.store);
            watch::<Box<dyn Material>>(&project.store);

            let (mesh, material) = (cube(4.0), tinted(0.9));
            let (mesh_uuid, material_uuid) = (mesh.uuid(), material.uuid());
            assert!(world.set_component(entity, mesh), "the MeshRef is replaced");
            assert!(
                world.set_component(entity, material),
                "the MaterialRef is replaced"
            );
            let visited = project.counted_frame(&mut world);

            assert_eq!(visited, 2, "the two changed references are read");
            assert!(
                project.stored::<Mesh>(&mesh_uuid).is_some(),
                "the new mesh copy is in the store"
            );
            assert!(
                project
                    .stored::<Box<dyn Material>>(&material_uuid)
                    .is_some(),
                "the new material copy is in the store"
            );
            assert_eq!(
                arrived::<Mesh>(&project.store),
                vec![Arrival {
                    uuid: mesh_uuid,
                    kind: ArrivalKind::Landed,
                }],
                "the new mesh lands under its uuid"
            );
            assert_eq!(
                arrived::<Box<dyn Material>>(&project.store),
                vec![Arrival {
                    uuid: material_uuid,
                    kind: ArrivalKind::Landed,
                }],
                "the new material lands under its uuid"
            );
        }

        /// Once every reference is resolved, the resolver reads no reference row.
        ///
        /// Not on a frame where nothing changed, nor on one where only another
        /// component of the entities did.
        #[test]
        fn an_unchanged_reference_is_not_visited() {
            let kmat_path = "materials/plain.kmat";
            let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
            let mut world = World::new();
            let asset_material = MaterialRef::Asset(AssetUUID::new_v5(kmat_path));
            let entities: Vec<EntityId> = (0..6)
                .map(|i| {
                    let material = if i % 2 == 0 {
                        tinted(i as f32 / 10.0)
                    } else {
                        asset_material.clone()
                    };
                    world.spawn((Transform::default(), cube(1.0 + i as f32), material))
                })
                .collect();
            project.settle(&mut world);
            project.counted_frame(&mut world);
            watch::<Mesh>(&project.store);
            watch::<Box<dyn Material>>(&project.store);

            for frame in 0..3 {
                assert_eq!(
                    project.counted_frame(&mut world),
                    0,
                    "frame {frame}: no reference changed, none is read"
                );
            }
            for (i, entity) in entities.iter().enumerate() {
                let mut transform = world
                    .get_mut::<Transform>(*entity)
                    .expect("the entity has a Transform");
                transform.translation = Vec3::new(i as f32, 0.0, 0.0);
            }
            assert_eq!(
                project.counted_frame(&mut world),
                0,
                "a moved entity's references are not read"
            );
            assert!(
                arrived::<Mesh>(&project.store).is_empty(),
                "no mesh copy is stored again"
            );
            assert!(
                arrived::<Box<dyn Material>>(&project.store).is_empty(),
                "no material copy is stored again"
            );
        }

        /// A material still decoding gets its copy into the store the frame its decode lands.
        ///
        /// The texture it names is asked for then, all without the reference
        /// changing. The landed copy reaches the store through the boundary's
        /// intake; the resolver still owes the material its textures.
        #[test]
        fn a_pending_decode_is_resolved_when_it_lands() {
            let tex_path = "textures/late.png";
            let kmat_path = "materials/late.kmat";
            let tex = AssetUUID::new_v5(tex_path);
            let mat = AssetUUID::new_v5(kmat_path);
            let project = Project::new(&[(tex_path, png(3, 2)), (kmat_path, kmat(0.4, Some(tex)))]);
            let mut world = World::new();
            world.spawn((cube(1.0), MaterialRef::Asset(mat)));

            assert_eq!(project.counted_frame(&mut world), 2, "the new references");
            assert!(
                project.stored::<Box<dyn Material>>(&mat).is_none(),
                "the premise: the material is still decoding"
            );
            assert_eq!(project.decode(), 1, "the material is filed for decoding");

            let visited = project.counted_frame(&mut world);
            assert_eq!(
                project.stored_roughness(&mat),
                Some(0.4),
                "the material is in the store the frame its decode lands"
            );
            assert_eq!(
                visited, 0,
                "the reference did not change: its row is not read"
            );
            assert_eq!(
                project.decode(),
                1,
                "the texture the landed material names is asked for"
            );

            project.counted_frame(&mut world);
            let texture = project
                .stored::<CpuTexture>(&tex)
                .expect("the material's texture is in the store once it lands");
            assert_eq!((texture.size.width, texture.size.height), (3, 2));
        }

        /// A material whose decode failed is not decoded again on the next
        /// frames; once its file changes it is, and its copy reaches the store.
        #[test]
        fn a_failed_decode_waits_for_its_file_to_change() {
            let kmat_path = "materials/broken.kmat";
            let mat = AssetUUID::new_v5(kmat_path);
            let project = Project::new(&[(kmat_path, b"definitely not a material".to_vec())]);
            let mut world = World::new();
            world.spawn(MaterialRef::Asset(mat));

            project.counted_frame(&mut world);
            assert_eq!(project.decode(), 1, "the material is filed for decoding");
            for frame in 0..4 {
                project.counted_frame(&mut world);
                assert_eq!(
                    project.decode(),
                    0,
                    "frame {frame}: the failed material is not filed again"
                );
                assert!(project.stored::<Box<dyn Material>>(&mat).is_none());
            }

            project.write(kmat_path, &kmat(0.7, None));
            project.invalidate(&mat);
            project.counted_frame(&mut world);
            assert_eq!(
                project.decode(),
                1,
                "the changed file is decoded again, the reference unchanged"
            );
            project.counted_frame(&mut world);
            assert_eq!(
                project.stored_roughness(&mat),
                Some(0.7),
                "the fixed material is in the store"
            );
        }

        /// No entity is given a CPU copy, and no component type exists to hold
        /// one: the store holds every copy, also after a hot reload.
        #[test]
        fn no_entity_carries_a_cpu_handle() {
            let kmat_path = "materials/plain.kmat";
            let mat = AssetUUID::new_v5(kmat_path);
            let project = Project::new(&[(kmat_path, kmat(0.5, None))]);
            let mut world = World::new();
            for name in [
                "HandleComponent<Mesh>",
                "HandleComponent<Box<dyn Material>>",
            ] {
                assert_eq!(
                    world.components().key_named(name),
                    None,
                    "`{name}` is not a registered component"
                );
            }
            let (inline, procedural) = (tinted(0.3), cube(2.0));
            let (inline_uuid, procedural_uuid) = (inline.uuid(), procedural.uuid());
            let entities = [
                world.spawn((procedural, MaterialRef::Asset(mat))),
                world.spawn((cube(3.0), inline)),
            ];
            project.settle(&mut world);

            assert!(project.stored::<Mesh>(&procedural_uuid).is_some());
            assert!(project.stored::<Box<dyn Material>>(&inline_uuid).is_some());
            assert_eq!(project.stored_roughness(&mat), Some(0.5));
            project.write(kmat_path, &kmat(0.8, None));
            project.invalidate(&mat);
            project.settle(&mut world);
            assert_eq!(
                project.stored_roughness(&mat),
                Some(0.8),
                "the reloaded material replaces its copy in the store"
            );
            for entity in entities {
                assert!(
                    world.get::<HandleComponent<Mesh>>(entity).is_none(),
                    "{entity:?} carries no CPU mesh"
                );
                assert!(
                    world
                        .get::<HandleComponent<Box<dyn Material>>>(entity)
                        .is_none(),
                    "{entity:?} carries no CPU material"
                );
            }
        }

        /// A `MaterialRef` added to an entity the resolver already read is a
        /// change: it is resolved on the next run, its row the only one read.
        #[test]
        fn a_reference_added_to_an_existing_entity_is_resolved() {
            let project = Project::new(&[]);
            let mut world = World::new();
            let entity = world.spawn(cube(1.0));
            project.counted_frame(&mut world);
            assert_eq!(project.counted_frame(&mut world), 0, "the premise: settled");

            let material = tinted(0.6);
            let uuid = material.uuid();
            world
                .add_component(entity, material)
                .expect("the MaterialRef is added");
            let visited = project.counted_frame(&mut world);

            assert_eq!(visited, 1, "the added reference is read");
            assert!(
                project.stored::<Box<dyn Material>>(&uuid).is_some(),
                "the added material's copy is in the store"
            );
        }

        /// What the resolver keeps for an asset once no reference names it.
        mod references_removed {
            use super::*;

            /// The assets the resolver still waits for, taken from the service.
            fn waited_for(project: &Project) -> HashSet<(AssetUUID, TypeId)> {
                project
                    .service
                    .lock()
                    .expect("the service's lock")
                    .take_resolving()
            }

            /// An asset that failed, or that no file holds, is not waited for
            /// once the last reference naming it is gone.
            #[test]
            #[ignore = "a known defect: the wanted set keeps assets no reference names; the generic resolver's liveness pruning (phase G2 of the plan) fixes it"]
            fn an_asset_no_reference_names_is_not_waited_for() {
                let broken_path = "materials/broken.kmat";
                let broken = AssetUUID::new_v5(broken_path);
                let missing = AssetUUID::new_v5("meshes/never_there.obj");
                let project = Project::new(&[(broken_path, b"not a material".to_vec())]);
                let mut world = World::new();
                let entity = world.spawn((MeshRef::Asset(missing), MaterialRef::Asset(broken)));
                project.counted_frame(&mut world);
                project.decode();
                project.counted_frame(&mut world);

                world.despawn(entity);
                project.counted_frame(&mut world);
                project.counted_frame(&mut world);

                assert_eq!(
                    waited_for(&project),
                    HashSet::new(),
                    "nothing names the failed material nor the missing mesh"
                );
            }

            /// A failed asset fixed on disk after its last reference left is not decoded.
            #[test]
            #[ignore = "a known defect: a failed asset no reference names is decoded again on any change; the event-driven wanted set (phase G2) fixes it"]
            fn a_fixed_asset_no_reference_names_is_not_decoded() {
                let kmat_path = "materials/broken.kmat";
                let mat = AssetUUID::new_v5(kmat_path);
                let project = Project::new(&[(kmat_path, b"not a material".to_vec())]);
                let mut world = World::new();
                let entity = world.spawn(MaterialRef::Asset(mat));
                project.counted_frame(&mut world);
                assert_eq!(project.decode(), 1, "the premise: filed once");
                project.counted_frame(&mut world);
                world.despawn(entity);
                project.counted_frame(&mut world);

                project.write(kmat_path, &kmat(0.7, None));
                project.invalidate(&mat);
                project.counted_frame(&mut world);

                assert_eq!(project.decode(), 0, "no reference names the material");
                project.counted_frame(&mut world);
                assert!(
                    project.stored::<Box<dyn Material>>(&mat).is_none(),
                    "no copy is made of what nothing names"
                );
            }

            /// An inline material edited again and again leaves only the copy its
            /// reference names in the store.
            #[test]
            #[ignore = "a known defect: no CPU copy is ever freed; the liveness rule over CPU copies (phase G5) fixes it"]
            fn an_edited_inline_material_leaves_no_copy_behind() {
                let project = Project::new(&[]);
                let mut world = World::new();
                let entity = world.spawn(tinted(0.0));
                project.counted_frame(&mut world);

                for edit in 1..=50 {
                    assert!(world.set_component(entity, tinted(edit as f32 / 100.0)));
                    project.counted_frame(&mut world);
                }

                let held = project
                    .store
                    .store::<Box<dyn Material>>()
                    .read()
                    .expect("the store's lock")
                    .len();
                assert_eq!(held, 1, "only the material the reference names is held");
            }
        }

        /// References resolved when the store's arrivals overflowed the resolver's reading.
        mod arrivals_lost {
            use super::*;

            /// Lists the project's folder again into its service, as a file
            /// appearing or leaving does.
            fn reindex(project: &Project) {
                let bytes = IndexBuilder::new(crate::trust::FileGuard::new(
                    project.dir.path(),
                    crate::trust::Trust::Project,
                    crate::trust::Refusals::detached(),
                ))
                .build_index_bytes()
                .expect("the project indexes");
                project
                    .service
                    .lock()
                    .expect("the service's lock")
                    .reindex(&bytes)
                    .expect("the index is replaced");
            }

            /// A mesh whose file left in a frame of more arrivals than the
            /// journal keeps is asked for again once its file is back.
            #[test]
            #[ignore = "a known defect: a Gone lost in a resync is never wanted again; the resolver's resync recovery (phase G2) fixes it"]
            fn a_mesh_gone_in_a_flood_of_arrivals_comes_back_with_its_file() {
                let obj_path = "meshes/back.obj";
                let mesh = AssetUUID::new_v5(obj_path);
                let triangle = b"v 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n".to_vec();
                let project = Project::new(&[(obj_path, triangle.clone())]);
                project
                    .service
                    .lock()
                    .expect("the service's lock")
                    .register_decoder::<Mesh>(
                        khora_core::asset::kinds::MESH.name(),
                        crate::asset::MeshDispatcher::default(),
                    );
                let mut world = World::new();
                world.spawn(MeshRef::Asset(mesh));
                project.settle(&mut world);
                assert!(project.stored::<Mesh>(&mesh).is_some(), "the premise");

                // A large scene's decodes land at the boundary the file leaves on.
                {
                    let other = AssetHandle::new(reconstruct_procedural_mesh(
                        ProceduralMeshKind::Cube,
                        [1.0, 0.0, 0.0, 0.0],
                    ));
                    let meshes = project.store.store::<Mesh>();
                    let mut meshes = meshes.write().expect("the store's lock");
                    for _ in 0..khora_data::assets::ARRIVALS_KEPT {
                        meshes.insert(AssetUUID::new(), other.clone());
                    }
                }
                fs::remove_file(project.dir.path().join(obj_path)).expect("removed");
                reindex(&project);
                project.invalidate(&mesh);
                project.counted_frame(&mut world);
                assert!(project.stored::<Mesh>(&mesh).is_none(), "the premise: gone");

                project.write(obj_path, &triangle);
                reindex(&project);
                project.invalidate(&mesh);
                project.settle(&mut world);

                assert!(
                    project.stored::<Mesh>(&mesh).is_some(),
                    "the mesh the entity names is back in the store"
                );
            }
        }
    }
}
