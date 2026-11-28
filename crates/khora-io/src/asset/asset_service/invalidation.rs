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

//! Invalidation of changed assets, their parts and the containers that read them,
//! each announced on the change channel; and the last good copies kept meanwhile.

use std::any::TypeId;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use khora_core::asset::{AssetHandle, AssetUUID, Stored};
use khora_core::event::Channel;
use khora_data::assets::Assets;

use super::AssetService;
use crate::asset::change::{AssetChanged, CHANGE_BACKLOG};

impl AssetService {
    /// Drops the cached copies of `uuid`; returns whether any storage held one.
    ///
    /// Its sub-assets and the containers reading its file are dropped with it,
    /// each path a container reads matched by the UUID the index gives it. Only
    /// an asset has a UUID: a changed file that is no asset (a glTF's `.bin`)
    /// reaches the containers reading it through
    /// [`invalidate_through`](Self::invalidate_through), by its path. Handles
    /// already out keep the old value. Each is announced on
    /// [`changes`](Self::changes) even when nothing was cached: after a failed
    /// reload, the engine still holds the last good copy.
    pub fn invalidate(&mut self, uuid: &AssetUUID) -> bool {
        let index = Arc::clone(&self.reader.index);
        self.invalidate_changed(*uuid, None, &|path| index.uuid_at(path))
    }

    /// Drops the cached copies of the changed file at `rel_fwd`; returns whether any were held.
    ///
    /// As [`invalidate`](Self::invalidate), its UUID and each path a container
    /// reads resolved by `uuid_of`, the project's identity registry the index
    /// is built through: a file made since the last reindex reaches the
    /// containers reading it.
    pub fn invalidate_through(
        &mut self,
        rel_fwd: &str,
        uuid_of: &dyn Fn(&str) -> AssetUUID,
    ) -> bool {
        self.invalidate_changed(uuid_of(rel_fwd), Some(rel_fwd), &|path| Some(uuid_of(path)))
    }

    /// Invalidates `uuid`, at `rel_fwd` when the index does not list it, and
    /// every container reading a path `uuid_of` gives one of the files dropped.
    fn invalidate_changed(
        &mut self,
        uuid: AssetUUID,
        rel_fwd: Option<&str>,
        uuid_of: &dyn Fn(&str) -> Option<AssetUUID>,
    ) -> bool {
        self.failed.clear();
        let mut any = false;
        let mut pending = vec![uuid];
        let mut seen = HashSet::new();
        while let Some(file) = pending.pop() {
            if !seen.insert(file) {
                continue;
            }
            any |= self.invalidate_file(&file);
            let key = (self.reader.index.get_metadata(&file))
                .map(|metadata| khora_core::asset::asset_key(&metadata.source_path))
                .or_else(|| rel_fwd.filter(|_| file == uuid).map(str::to_owned));
            // A decode in flight names the files it read beside its own by
            // key: the key moves with the file, under the uuid it gives.
            if let Some(key) = &key {
                self.generations.bump(&AssetUUID::new_v5(key));
            }
            // A path names the file when it resolves to the file's uuid: a
            // renamed file's old path, now another file's, never does.
            let names_file = |external: &String| uuid_of(external) == Some(file);
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

    /// Takes the changed assets whose new value is still being decoded, for `asset_reload`.
    pub(crate) fn take_reloading(&mut self) -> HashSet<AssetUUID> {
        std::mem::take(&mut self.reloading)
    }

    /// Keeps `uuids` as the changed assets still being decoded.
    pub(crate) fn keep_reloading(&mut self, uuids: HashSet<AssetUUID>) {
        self.reloading.extend(uuids);
    }

    /// Serves `handle`, the last good copy of `uuid`, until `uuid` is invalidated again.
    ///
    /// For an asset whose current bytes did not decode. Refused (`false`) for a
    /// copy a replaced service served, no copy of this service's asset.
    pub fn keep<A: Stored>(&mut self, uuid: &AssetUUID, handle: AssetHandle<A>) -> bool {
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

    /// Takes every asset announced since the last call, for the engine to replace its copies.
    ///
    /// After an overflow, every asset this service ever served is returned. The
    /// single consumer is the `asset_reload` `DataSystem`.
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

    /// Returns the stream of assets whose bytes changed, sent by [`invalidate`](Self::invalidate).
    ///
    /// Drained by its single consumer, the `asset_reload` `DataSystem`.
    pub fn changes(&self) -> &Channel<AssetChanged> {
        &self.changes
    }

    /// Announces every asset `outgoing`, the service this one replaces, ever served.
    ///
    /// The engine's copies of them may be stale. They are inherited: an overflow
    /// still names them, and none is kept as a last good copy of this service's.
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

/// Sends `AssetChanged(uuid)` on `changes`, noting in `lost` if a full stream dropped one.
fn announce(changes: &Channel<AssetChanged>, lost: &AtomicBool, uuid: AssetUUID) {
    let dropped = changes.dropped();
    changes.send(AssetChanged(uuid));
    if changes.dropped() != dropped {
        lost.store(true, Ordering::Relaxed);
    }
}
