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

//! Invalidation: dropping a changed asset, the parts it served and the
//! containers that read it, announcing each on the change channel, and the
//! last good copies served while a changed asset is decoded again.

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_core::event::Channel;
use khora_data::assets::Assets;

use super::AssetService;
use crate::asset::change::{AssetChanged, CHANGE_BACKLOG};

impl AssetService {
    /// Drops the cached handle for `uuid` across every typed storage.
    /// Subsequent `load::<A>()` calls re-run the IO + decoder pipeline.
    /// Returns `true` if any storage held this UUID.
    ///
    /// Outstanding clones of the previously-cached `AssetHandle<A>` keep the
    /// old asset alive until they themselves drop — by design (in-flight
    /// readers don't see a half-loaded replacement).
    ///
    /// Invalidating a container file also drops its opened container and
    /// every sub-asset extracted from it.
    ///
    /// `uuid` and every sub-asset it served are announced on
    /// [`changes`](Self::changes) whether or not anything was cached: after
    /// a reload that failed, the service holds nothing while the engine
    /// still holds the last good copy, which the next change must reach.
    ///
    /// A container that reads `uuid`'s file besides its own (a glTF's `.bin`
    /// or image beside it) is invalidated with it.
    pub fn invalidate(&mut self, uuid: &AssetUUID) -> bool {
        self.failed.clear();
        let mut any = false;
        let mut pending = vec![*uuid];
        let mut seen = HashSet::new();
        while let Some(file) = pending.pop() {
            if !seen.insert(file) {
                continue;
            }
            any |= self.invalidate_file(&file);
            // A file the index does not list yet (created since the last
            // reindex) is known only by its uuid, the one its path gives.
            let key = self
                .reader
                .index
                .get_metadata(&file)
                .map(|metadata| khora_core::asset::asset_key(&metadata.source_path));
            // A decode in flight names the files it read beside its own by
            // key: the key moves with the file, under the uuid it gives.
            if let Some(key) = &key {
                self.generations.bump(&AssetUUID::new_v5(key));
            }
            let names_file = |external: &String| {
                key.as_ref() == Some(external) || AssetUUID::new_v5(external) == file
            };
            pending.extend(
                self.external_files
                    .iter()
                    .filter(|(_, files)| files.iter().any(names_file))
                    .map(|(container, _)| *container),
            );
        }
        any
    }

    /// Drops and announces `uuid` and every sub-asset it served.
    fn invalidate_file(&mut self, uuid: &AssetUUID) -> bool {
        let mut any = self.opened.remove(uuid).is_some();
        let subs = self.served_subs.get(uuid);
        for target in std::iter::once(uuid).chain(subs.into_iter().flatten()) {
            // A decode in flight that read it is of the bytes before the
            // change: it is never installed.
            self.generations.bump(target);
            for storage in self.storages.values_mut() {
                if storage.remove_uuid(target) {
                    any = true;
                }
            }
            announce(&self.changes, &self.changes_lost, *target);
        }
        any
    }

    /// The changed assets whose new value is still being decoded, taken:
    /// what the `asset_reload` system refreshes once each lands.
    pub(crate) fn take_reloading(&mut self) -> HashSet<AssetUUID> {
        std::mem::take(&mut self.reloading)
    }

    /// Keeps `uuids` as the changed assets still being decoded.
    pub(crate) fn keep_reloading(&mut self, uuids: HashSet<AssetUUID>) {
        self.reloading.extend(uuids);
    }

    /// Serves `handle` for `uuid` until `uuid` is invalidated again: the
    /// last good copy of an asset whose current bytes did not decode, so a
    /// reference resolved meanwhile shows what the engine already shows.
    ///
    /// Refused (returns `false`) for a copy a replaced service served, which
    /// is no copy of this service's asset.
    pub fn keep<A: Asset>(&mut self, uuid: &AssetUUID, handle: AssetHandle<A>) -> bool {
        if self.inherited_mut().contains(uuid) {
            return false;
        }
        let storage = self
            .storages
            .entry(TypeId::of::<A>())
            .or_insert_with(|| Box::new(Assets::<A>::new()));
        if let Some(assets) = storage.as_any_mut().downcast_mut::<Assets<A>>() {
            assets.insert(*uuid, handle);
            self.served.insert(*uuid);
            return true;
        }
        false
    }

    /// Takes every asset announced since the last call, for the engine to
    /// replace its copies. If the stream overflowed since, the announcements
    /// it lost cannot be told apart, so every asset this service ever served
    /// is returned.
    ///
    /// The stream's single consumer is the `asset_reload` DataSystem.
    pub fn take_changes(&self) -> HashSet<AssetUUID> {
        let mut changed: HashSet<AssetUUID> = self
            .changes
            .drain()
            .into_iter()
            .map(|AssetChanged(uuid)| uuid)
            .collect();
        if self.changes_lost.swap(false, Ordering::Relaxed) {
            log::warn!(
                "AssetService: more than {CHANGE_BACKLOG} assets changed at once; every asset \
                 served is reloaded"
            );
            changed.extend(self.served.iter().copied());
            changed.extend(self.inherited_mut().iter().copied());
        }
        changed
    }

    /// The stream of assets whose bytes changed: every [`invalidate`](Self::invalidate)
    /// sends the invalidated uuid and each part it served.
    ///
    /// Drained by its single consumer, the `asset_reload` DataSystem.
    pub fn changes(&self) -> &Channel<AssetChanged> {
        &self.changes
    }

    /// Announces, on this service's [`changes`](Self::changes), every asset
    /// `outgoing` ever served: this service replaces it, so the copies the
    /// engine took from it may be stale.
    ///
    /// What `outgoing` served (and what it inherited in turn) is inherited:
    /// a stream overflow still names it, and it is never kept as a last good
    /// copy of this service's asset.
    pub fn announce_replacing(&self, outgoing: &AssetService) {
        let mut replaced: HashSet<AssetUUID> = outgoing.inherited_mut().clone();
        replaced.extend(outgoing.served.iter().copied());
        for uuid in &replaced {
            announce(&self.changes, &self.changes_lost, *uuid);
        }
        let mut inherited = self.inherited_mut();
        inherited.extend(
            replaced
                .into_iter()
                .filter(|uuid| !self.served.contains(uuid)),
        );
    }
}

/// Sends `AssetChanged(uuid)` on `changes`, noting in `lost` if the full
/// stream dropped one to make room.
fn announce(changes: &Channel<AssetChanged>, lost: &AtomicBool, uuid: AssetUUID) {
    let dropped = changes.dropped();
    changes.send(AssetChanged(uuid));
    if changes.dropped() != dropped {
        lost.store(true, Ordering::Relaxed);
    }
}
