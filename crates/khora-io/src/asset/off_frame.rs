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

//! Assets decoded off the frame: a missing asset is filed once, decoded by the `loading`
//! family, and taken in at a frame's boundary unless a file it read changed meanwhile.

use std::any::{Any, TypeId};
use std::sync::{Arc, Mutex};

use khora_core::asset::{
    Asset, AssetDecoding, AssetHandle, AssetSource, AssetUUID, DecodeRequest, Decoded,
};
use khora_core::event::{Channel, WhenFull};
use khora_core::lane::Lease;
use khora_core::Runtime;
use khora_data::assets::{AssetStore, Assets};

use super::requested::Requested;
use super::AssetService;

/// How a decoded value of one type goes into the service's cache and the
/// engine's store.
pub(super) type Installer = fn(&mut AssetService, &AssetStore, AssetUUID, Box<dyn Any + Send>);

impl AssetService {
    pub(super) fn request_off_frame<A: Asset>(&mut self, uuid: &AssetUUID) -> Requested<A> {
        match self.request_decoded::<A>(uuid) {
            // Being decoded again after a change: the last good copy serves
            // meanwhile.
            Requested::Pending => self
                .meanwhile
                .get(&(*uuid, TypeId::of::<A>()))
                .and_then(|copy| copy.downcast_ref::<AssetHandle<A>>())
                .map_or(Requested::Pending, |copy| Requested::Ready(copy.clone())),
            requested => requested,
        }
    }

    /// Answers as [`request`](Self::request), without the last good copy served meanwhile.
    ///
    /// So it says whether a reloaded asset's new value landed, as `asset_reload` asks.
    pub(crate) fn request_decoded<A: Asset>(&mut self, uuid: &AssetUUID) -> Requested<A> {
        let type_id = TypeId::of::<A>();
        let storage = self
            .storages
            .entry(type_id)
            .or_insert_with(|| Box::new(Assets::<A>::new()));
        if let Some(handle) = storage
            .as_any_mut()
            .downcast_mut::<Assets<A>>()
            .and_then(|assets| assets.get(uuid))
        {
            return Requested::Ready(handle.clone());
        }
        if self.failed.contains(uuid) {
            return Requested::Failed;
        }
        self.installers.entry(type_id).or_insert(install::<A>);
        // Filed once while its decode is out; again only once a lease turned
        // that decode down — it read a file that changed since.
        let key = (*uuid, type_id);
        let abandoned = self.generations.take_abandoned(*uuid, type_id);
        if !self.pending.contains(&key) || abandoned {
            self.pending.insert(key);
            let dropped = self.requests.dropped();
            self.requests.send(DecodeRequest {
                uuid: *uuid,
                type_id,
                generation: self.generations.now(),
            });
            // Refused by a full backlog: asked for again on a later frame.
            if self.requests.dropped() != dropped {
                self.pending.remove(&(*uuid, type_id));
                log::warn!("asset {uuid}: the decode backlog is full, asked for again later");
            }
        }
        Requested::Pending
    }

    /// Takes in one decode the `loading` family staged, at a frame's boundary.
    ///
    /// The value goes into this service's cache and its handle into `store`; a
    /// failure is answered [`Requested::Failed`] until an asset changes.
    pub fn take_in(&mut self, decoded: Decoded, store: &AssetStore) {
        let Decoded {
            uuid,
            type_id,
            value,
            external_files,
        } = decoded;
        self.pending.remove(&(uuid, type_id));
        let value = match value {
            Ok(value) => value,
            Err(error) => {
                log::warn!("asset {uuid} could not be decoded: {error}");
                self.failed.insert(uuid);
                return;
            }
        };
        let Some(install) = self.installers.get(&type_id).copied() else {
            log::warn!("asset {uuid} was decoded as a type nothing asked for; dropped");
            return;
        };
        for (file, read) in external_files {
            self.external_files.insert(file, read);
        }
        if let Some(container) = self.container_of(&uuid) {
            self.served_subs.entry(container).or_default().insert(uuid);
        }
        install(self, store, uuid, value);
    }

    /// Serves `handle`, the engine's copy of `uuid`, while `uuid` is decoded again.
    ///
    /// Not a copy a replaced service served: that is no copy of this project's asset.
    pub(crate) fn serve_meanwhile<A: Asset>(&mut self, uuid: &AssetUUID, handle: AssetHandle<A>) {
        if self.inherited_mut().contains(uuid) {
            return;
        }
        self.meanwhile
            .insert((*uuid, TypeId::of::<A>()), Box::new(handle));
    }

    /// Stops serving a last good copy of `uuid` as an `A`, once its reload has settled.
    pub(crate) fn settle_meanwhile<A: Asset>(&mut self, uuid: &AssetUUID) {
        self.meanwhile.remove(&(*uuid, TypeId::of::<A>()));
    }

    /// Takes over the decode request channel of `outgoing`, the service it replaces.
    ///
    /// A service swapped into the engine's place so keeps reaching the `loading` family.
    pub fn adopt_decode_requests(&mut self, outgoing: &mut AssetService) {
        std::mem::swap(&mut self.requests, &mut outgoing.requests);
        // The channel's stamps come of `outgoing`'s counter, so this service
        // counts on it; what was in flight read the other project, so none of
        // it is installed and every request is filed afresh.
        std::mem::swap(&mut self.generations, &mut outgoing.generations);
        self.generations.bump_all();
        outgoing.pending.clear();
        self.pending.clear();
    }

    /// Files this service's later decode requests on `requests`, its `loading` family's.
    ///
    /// What it had filed elsewhere is asked for again; no decode in flight is installed.
    pub(super) fn use_decode_requests(
        &mut self,
        requests: Channel<DecodeRequest>,
        generations: Arc<super::generations::Generations>,
    ) {
        self.requests = requests;
        // Stamps on that channel come of the counter its family serves:
        // this service counts on it too, and nothing in flight before is
        // installed.
        self.generations = generations;
        self.generations.bump_all();
        self.pending.clear();
    }

    /// Returns the container file of `uuid`, when it is a sub-asset.
    fn container_of(&self, uuid: &AssetUUID) -> Option<AssetUUID> {
        match self.reader.source_of(uuid) {
            Ok((_, AssetSource::Sub { container, .. })) => Some(*container),
            _ => None,
        }
    }
}

/// Puts a decoded `A` into `service`'s cache and `store`, counted as served.
fn install<A: Asset>(
    service: &mut AssetService,
    store: &AssetStore,
    uuid: AssetUUID,
    value: Box<dyn Any + Send>,
) {
    let Ok(asset) = value.downcast::<A>() else {
        log::warn!("asset {uuid} was decoded as another type than asked for; dropped");
        return;
    };
    // A synchronous load of it landed first: that handle stays the one
    // served, so whoever holds it and the store agree.
    let cached = service
        .storages
        .get_mut(&TypeId::of::<A>())
        .and_then(|storage| storage.as_any_mut().downcast_mut::<Assets<A>>())
        .and_then(|assets| assets.get(&uuid).cloned());
    let fresh = cached.is_none();
    let handle = cached.unwrap_or_else(|| AssetHandle::new(*asset));
    service.failed.remove(&uuid);
    service.served.insert(uuid);
    service.inherited_mut().remove(&uuid);
    if let Some(assets) = service
        .storages
        .get_mut(&TypeId::of::<A>())
        .and_then(|storage| storage.as_any_mut().downcast_mut::<Assets<A>>())
    {
        assets.insert(uuid, handle.clone());
    }
    // The new value serves in place of the last good copy.
    service.settle_meanwhile::<A>(&uuid);
    if fresh {
        service.load_count += 1;
    }
    match store.store::<A>().write() {
        Ok(mut stored) => stored.insert(uuid, handle),
        Err(_) => log::error!("the asset store is poisoned; asset {uuid} is not stored"),
    }
}

/// The `loading` family's [`AssetDecoding`] over the service a shared handle holds.
///
/// It takes that service's reader under the lock, briefly, and decodes without it.
pub(super) struct ServiceDecoding {
    service: Arc<Mutex<AssetService>>,
}

impl ServiceDecoding {
    pub(super) fn new(service: &Arc<Mutex<AssetService>>) -> Self {
        Self {
            service: Arc::clone(service),
        }
    }

    fn parts(
        &self,
    ) -> (
        Arc<super::AssetReader>,
        Arc<super::generations::Generations>,
    ) {
        let service = self
            .service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        (service.reader(), Arc::clone(&service.generations))
    }
}

impl AssetDecoding for ServiceDecoding {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        let (reader, _) = self.parts();
        reader.decode(uuid, type_id)
    }

    fn decode_leased(&self, request: &DecodeRequest) -> (Decoded, Option<Lease>) {
        let (reader, generations) = self.parts();
        let decoded = reader.decode(&request.uuid, request.type_id);
        let (filed, uuid, type_id) = (request.generation, request.uuid, request.type_id);
        // A decode turned down is noted, so the next request for it files it
        // again: the one lost here is the only one out.
        let lease = if decoded.value.is_ok() {
            // Holds while none of the files it read changed: its own, each
            // container it opened, and the files beside them.
            let mut read = vec![request.uuid];
            for (container, files) in &decoded.external_files {
                read.push(*container);
                read.extend(files.iter().map(|file| AssetUUID::new_v5(file)));
            }
            Lease::new(move || {
                let holds = generations.none_changed_since(read.iter().copied(), filed);
                if !holds {
                    generations.abandon(uuid, type_id);
                }
                holds
            })
        } else {
            // A failure may come of a file it never got to read — a buffer
            // missing beside its container: any change asks again.
            Lease::new(move || {
                let holds = generations.nothing_changed_since(filed);
                if !holds {
                    generations.abandon(uuid, type_id);
                }
                holds
            })
        };
        (decoded, Some(lease))
    }
}

/// The decode request channel and the asset service the `loading` family serves.
///
/// The runtime's service is attached at the boot and at every frame boundary, so
/// one inserted or replaced later is served from the next boundary. Until one
/// is, every decode fails and says why. Its lock is a leaf: held only to read
/// or replace the attached service, never while that service's lock is taken.
pub struct DecodeHub {
    requests: Channel<DecodeRequest>,
    /// The change counter every service attached here counts on, so every
    /// stamp on the channel compares with every change.
    generations: Arc<super::generations::Generations>,
    attached: Mutex<Option<Arc<Mutex<AssetService>>>>,
}

impl DecodeHub {
    fn new() -> Self {
        Self {
            requests: Channel::bounded(super::asset_service::REQUEST_BACKLOG, WhenFull::Reject),
            generations: Arc::new(super::generations::Generations::default()),
            attached: Mutex::new(None),
        }
    }

    /// Returns the attached service, if any.
    fn service(&self) -> Option<Arc<Mutex<AssetService>>> {
        self.attached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Attaches the asset service `runtime` holds, unless it is attached already.
    ///
    /// It then files its decode requests on this hub's channel.
    fn attach(&self, runtime: &Runtime) {
        let Some(service) = runtime.services.get::<Arc<Mutex<AssetService>>>().cloned() else {
            return;
        };
        if self
            .service()
            .is_some_and(|attached| Arc::ptr_eq(&attached, &service))
        {
            return;
        }
        service
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .use_decode_requests(self.requests.clone(), Arc::clone(&self.generations));
        *self
            .attached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(service);
    }
}

impl AssetDecoding for DecodeHub {
    fn decode(&self, uuid: &AssetUUID, type_id: TypeId) -> Decoded {
        match self.service() {
            Some(service) => ServiceDecoding::new(&service).decode(uuid, type_id),
            None => no_service(uuid, type_id),
        }
    }

    fn decode_leased(&self, request: &DecodeRequest) -> (Decoded, Option<Lease>) {
        match self.service() {
            Some(service) => ServiceDecoding::new(&service).decode_leased(request),
            None => (no_service(&request.uuid, request.type_id), None),
        }
    }
}

/// Returns the failed decode of `uuid` when the engine holds no asset service.
fn no_service(uuid: &AssetUUID, type_id: TypeId) -> Decoded {
    Decoded {
        uuid: *uuid,
        type_id,
        value: Err("the engine holds no asset service".to_owned()),
        external_files: Vec::new(),
    }
}

/// Installs one [`DecodeHub`] in `runtime`, unless one is there, attaching its asset service.
///
/// It is installed as the request channel and the [`AssetDecoding`] the `loading`
/// family reads.
pub fn install_decoding(runtime: &mut Runtime) {
    if runtime.resources.get::<Arc<DecodeHub>>().is_some() {
        return;
    }
    let hub = Arc::new(DecodeHub::new());
    hub.attach(runtime);
    runtime.resources.insert(hub.requests.clone());
    let decoding: Arc<dyn AssetDecoding> = Arc::clone(&hub) as Arc<dyn AssetDecoding>;
    runtime.resources.insert(decoding);
    runtime.resources.insert(hub);
}

/// Attaches the asset service `runtime` holds to its decode hub, if it is a new one.
///
/// A frame boundary calls it before it takes decodes in. Does nothing without a hub.
pub fn attach_decoding(runtime: &Runtime) {
    if let Some(hub) = runtime.resources.get::<Arc<DecodeHub>>() {
        hub.attach(runtime);
    }
}
