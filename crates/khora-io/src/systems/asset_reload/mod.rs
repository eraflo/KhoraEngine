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

//! The asset reload: every copy the engine took of a changed asset replaced
//! under the same uuid in the asset store.
//!
//! The new value is decoded off the frame; until it lands, the last good copy
//! keeps serving. An asset the index says is gone is dropped at once. The
//! store's arrivals tell the residencies, which make the GPU copies again.

use std::any::TypeId;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use khora_core::asset::font::Font;
use khora_core::asset::{AssetHandle, AssetUUID, Material, Stored};
use khora_core::ecs::entity::EntityId;
use khora_core::lane::OutputDeck;
use khora_core::renderer::api::gpu_scene::Mesh;
use khora_core::renderer::api::resource::CpuTexture;
use khora_core::sync::{LockExt, RwLockExt};
use khora_core::Runtime;
use khora_data::ecs::{
    Component, DataSystemRegistration, HandleComponent, SystemContext, TickPhase, World,
};
use khora_data::AssetStore;

use super::asset_resolver::load_textures;
use crate::asset::{AssetService, Requested};

/// What decoding a changed asset again gave.
enum Reloaded<A: Stored> {
    /// The asset's current value.
    Fresh(AssetHandle<A>),
    /// The asset left the index, or its container stopped holding it: every copy goes.
    Gone,
    /// The asset did not decode (a file caught mid-write): the last good copy
    /// stays, in the service too, until the next change.
    Kept,
    /// Its new value is still being decoded off the frame: the last good
    /// copy keeps serving until it lands.
    Pending,
}

// Written out rather than derived: a derive would demand `A: Clone`, which
// an `AssetHandle` never needs.
impl<A: Stored> Clone for Reloaded<A> {
    fn clone(&self) -> Self {
        match self {
            Self::Fresh(handle) => Self::Fresh(handle.clone()),
            Self::Gone => Self::Gone,
            Self::Kept => Self::Kept,
            Self::Pending => Self::Pending,
        }
    }
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

    let reload = Reload {
        service,
        store: runtime.resources.get::<AssetStore>(),
        pending: RefCell::new(HashSet::new()),
    };
    reload.textures(&changed);
    let held_materials = reload.materials(&changed);
    let held_meshes = reload.meshes(&changed);
    let held_fonts = reload.fonts(world, &changed);

    // A changed asset of which no copy is held: the resolver decodes it
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
        reload.forget::<Font>(uuid);
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

/// The services one reload pass works through.
struct Reload<'a> {
    service: &'a Mutex<AssetService>,
    store: Option<&'a AssetStore>,
    /// The changed assets whose new value is still being decoded.
    pending: RefCell<HashSet<AssetUUID>>,
}

impl Reload<'_> {
    /// Asks the service for `uuid` again, decoded off the frame.
    ///
    /// `last_good` is the copy the engine shows meanwhile.
    fn reload<A: Stored>(&self, uuid: &AssetUUID, last_good: &AssetHandle<A>) -> Reloaded<A> {
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
            // The decode failed: a part its container stopped holding is
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
    ///
    /// The material residency makes the materials naming one again.
    fn textures(&self, changed: &HashSet<AssetUUID>) {
        let Some(store) = self.store else {
            return;
        };
        let textures = store.store::<CpuTexture>();
        let held: Vec<(AssetUUID, AssetHandle<CpuTexture>)> = {
            let guard = textures.read_recovered();
            changed
                .iter()
                .filter_map(|uuid| guard.get(uuid).map(|texture| (*uuid, texture.clone())))
                .collect()
        };
        for (uuid, last_good) in held {
            match self.reload::<CpuTexture>(&uuid, &last_good) {
                Reloaded::Fresh(texture) => self.publish(uuid, &texture),
                Reloaded::Gone => {
                    textures.write_recovered().remove(&uuid);
                }
                Reloaded::Kept | Reloaded::Pending => {}
            }
        }
    }

    /// Replaces in the store every changed material it holds; returns those uuids.
    ///
    /// The textures a stored material names are asked for when one of them
    /// changed and the store lacks it; the resolver follows them until they land.
    fn materials(&self, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
        let Some(store) = self.store else {
            return HashSet::new();
        };
        let mut affected = Vec::new();
        let mut naming = Vec::new();
        {
            let materials = store.store::<Box<dyn Material>>();
            let held = materials.read_recovered();
            for uuid in held.keys() {
                let Some(material) = held.get(&uuid) else {
                    continue;
                };
                if changed.contains(&uuid) {
                    affected.push((uuid, material.clone()));
                } else if material
                    .textures()
                    .iter()
                    .any(|texture| changed.contains(texture))
                {
                    naming.push(material.clone());
                }
            }
        }

        let mut held = HashSet::new();
        for (uuid, current) in affected {
            held.insert(uuid);
            match self.reload::<Box<dyn Material>>(&uuid, &current) {
                Reloaded::Fresh(material) => self.publish(uuid, &material),
                Reloaded::Gone => self.forget::<Box<dyn Material>>(&uuid),
                Reloaded::Kept | Reloaded::Pending => naming.push(current),
            }
        }
        for material in naming {
            self.load_material_textures(&**material);
        }
        held
    }

    /// Replaces in the store every changed mesh it holds; returns those uuids.
    fn meshes(&self, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
        let Some(store) = self.store else {
            return HashSet::new();
        };
        let affected: Vec<(AssetUUID, AssetHandle<Mesh>)> = {
            let meshes = store.store::<Mesh>();
            let held = meshes.read_recovered();
            changed
                .iter()
                .filter_map(|uuid| held.get(uuid).map(|mesh| (*uuid, mesh.clone())))
                .collect()
        };
        let mut held = HashSet::new();
        for (uuid, current) in affected {
            held.insert(uuid);
            match self.reload::<Mesh>(&uuid, &current) {
                Reloaded::Fresh(mesh) => self.publish(uuid, &mesh),
                Reloaded::Gone => self.forget::<Mesh>(&uuid),
                Reloaded::Kept | Reloaded::Pending => {}
            }
        }
        held
    }

    /// Replaces every entity's resolved font that changed, under its uuid.
    ///
    /// The handle written again tells the text systems to lay their texts out
    /// anew. Returns the changed fonts some entity holds.
    fn fonts(&self, world: &mut World, changed: &HashSet<AssetUUID>) -> HashSet<AssetUUID> {
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

    /// Asks for the textures `material` names that the store lacks.
    ///
    /// The resolver follows those not landed yet until they do.
    fn load_material_textures(&self, material: &dyn Material) {
        let Some(store) = self.store else {
            return;
        };
        let mut svc = self.service.lock_recovered();
        let waiting = load_textures(material, &store.store::<CpuTexture>(), &mut svc);
        svc.keep_resolving(
            waiting
                .into_iter()
                .map(|uuid| (uuid, TypeId::of::<CpuTexture>())),
        );
    }

    /// Stores `handle` as the copy of `uuid`, unless it is the one held.
    fn publish<A: Stored>(&self, uuid: AssetUUID, handle: &AssetHandle<A>) {
        if let Some(store) = self.store {
            let copies = store.store::<A>();
            let mut copies = copies.write_recovered();
            if copies.get(&uuid) != Some(handle) {
                copies.insert(uuid, handle.clone());
            }
        }
    }

    /// Drops the shared copy of `uuid`.
    fn forget<A: Stored>(&self, uuid: &AssetUUID) {
        if let Some(store) = self.store {
            store.store::<A>().write_recovered().remove(uuid);
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
        // before the resolver reads the references and the store's arrivals.
        order_hint: -6,
        runs_after: &[],
        observes: &[],
        role: khora_data::ecs::Role::Upkeep,
    }
}

#[cfg(test)]
mod tests;
