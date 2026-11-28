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

//! The asset resolver: what the references (an entity's mesh and material
//! [`AssetRef`]s, a UI font or image) name, put into the asset store under the
//! reference's uuid.
//!
//! See the book: concepts/assets.md, "From reference to copy".

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

use khora_core::asset::{Asset, AssetHandle, AssetUUID, LocalSource, Material, Stored};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::Runtime;
use khora_data::assets::{ArrivalKind, ArrivalsRead, Assets};
use khora_data::ecs::{
    AssetRef, Changed, ComponentKey, DataSystemRegistration, SystemContext, Tick, TickPhase, World,
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
/// Returns how many `AssetRef<Mesh>` and `AssetRef<Box<dyn Material>>` rows it read, each counting
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
    let read = resolver.references_changed::<Box<dyn Material>>(world, since, "material")
        + resolver.references_changed::<Mesh>(world, since, "mesh");
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
    fn ask<A: Stored>(&mut self, svc: &mut AssetService, uuid: AssetUUID) {
        match svc.request::<A>(&uuid) {
            Requested::Ready(handle) => publish(&self.store.store::<A>(), uuid, &handle),
            Requested::Pending | Requested::Failed => {
                self.wanted.insert((uuid, TypeId::of::<A>()));
            }
        }
    }

    /// Resolves every `AssetRef<A>` changed since `since`; returns how many it read.
    ///
    /// A local source (a procedural mesh, an inline material) is built and
    /// stored under its uuid unless the store holds it; an asset is asked of
    /// the service. A value made in code the store does not hold is logged:
    /// nothing draws it.
    fn references_changed<A: Asset>(&mut self, world: &World, since: Tick, what: &str) -> usize {
        let store = self.store.store::<A>();
        let mut read = 0;
        let mut local: Vec<(AssetUUID, A::LocalSource)> = Vec::new();
        let mut assets: Vec<(EntityId, AssetUUID)> = Vec::new();
        {
            let held = store.read_recovered();
            let changed = world
                .query::<(EntityId, &AssetRef<A>, Changed<AssetRef<A>>)>()
                .since(since);
            for (entity, reference, _) in changed {
                read += 1;
                match reference {
                    AssetRef::Local(source) => {
                        if !held.contains(&reference.uuid()) {
                            local.push((reference.uuid(), source.clone()));
                        }
                    }
                    AssetRef::Asset(uuid) => assets.push((entity, *uuid)),
                    AssetRef::Made(uuid) => {
                        if !held.contains(uuid) {
                            log::warn!(
                                "asset_resolver: entity {entity:?} names a {what} made in code \
                                 ({uuid:?}) the asset store does not hold; nothing draws it"
                            );
                        }
                    }
                }
            }
        }
        for (uuid, source) in local {
            let mut held = store.write_recovered();
            if !held.contains(&uuid) {
                held.insert(uuid, AssetHandle::new(source.build()));
            }
        }
        self.ask_for_assets::<A>(&assets, what);
        read
    }

    /// Asks the service for each asset of `assets`, named by the entity beside it.
    fn ask_for_assets<A: Stored>(&mut self, assets: &[(EntityId, AssetUUID)], what: &str) {
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
fn publish<A: Stored>(store: &RwLock<Assets<A>>, uuid: AssetUUID, handle: &AssetHandle<A>) {
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
        observes: &[ComponentKey::of::<AssetRef<Mesh>>, ComponentKey::of::<AssetRef<Box<dyn Material>>>],
        role: khora_data::ecs::Role::Upkeep,
    }
}

#[cfg(test)]
mod tests;
