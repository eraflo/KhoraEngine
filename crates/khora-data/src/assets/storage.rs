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

//! A generic, type-safe storage for loaded asset handles.

use std::collections::HashMap;
use std::sync::Mutex;

use khora_core::asset::{Asset, AssetHandle, AssetUUID};
use khora_core::sync::LockExt;

use crate::assets::journal::Journal;
use crate::assets::{Arrival, ArrivalKind, ArrivalsRead, ARRIVALS_KEPT};

/// The in-memory cache of loaded assets of type `A`, keyed by UUID.
///
/// One handle per UUID: every request for a cached asset gets a clone of it.
/// Each insert and removal is recorded as an [`Arrival`], which every
/// reader of [`arrivals`](Self::arrivals) reads once: whoever writes the
/// store tells its readers by writing it.
pub struct Assets<A: Asset> {
    storage: HashMap<AssetUUID, AssetHandle<A>>,
    /// The arrivals not every reader has read; a leaf lock, so a reader
    /// holding the store's read lock can read them.
    journal: Mutex<Journal>,
}

impl<A: Asset> Default for Assets<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: Asset> Clone for Assets<A> {
    /// Copies the handles; the clone's journal is empty and has no reader.
    fn clone(&self) -> Self {
        let kept = self.journal.lock_recovered().kept();
        Self {
            storage: self.storage.clone(),
            journal: Mutex::new(Journal::new(kept)),
        }
    }
}

impl<A: Asset> Assets<A> {
    /// Creates a new, empty asset storage whose journal keeps [`ARRIVALS_KEPT`] entries.
    pub fn new() -> Self {
        Self::with_arrivals_kept(ARRIVALS_KEPT)
    }

    /// Creates an empty asset storage whose arrivals journal keeps at most `kept` entries.
    pub fn with_arrivals_kept(kept: usize) -> Self {
        Self {
            storage: HashMap::new(),
            journal: Mutex::new(Journal::new(kept)),
        }
    }

    /// Caches `handle` under `uuid`, replacing any handle already there.
    ///
    /// Recorded as [`ArrivalKind::Landed`] for a uuid not held, else
    /// [`ArrivalKind::Replaced`].
    pub fn insert(&mut self, uuid: AssetUUID, handle: AssetHandle<A>) {
        let kind = match self.storage.insert(uuid, handle) {
            Some(_) => ArrivalKind::Replaced,
            None => ArrivalKind::Landed,
        };
        self.journal.lock_recovered().record(Arrival { uuid, kind });
    }

    /// Returns the handle cached under `uuid`, if any.
    pub fn get(&self, uuid: &AssetUUID) -> Option<&AssetHandle<A>> {
        self.storage.get(uuid)
    }

    /// Checks if an asset with the specified UUID exists in the storage.
    pub fn contains(&self, uuid: &AssetUUID) -> bool {
        self.storage.contains_key(uuid)
    }

    /// Drops the cached handle for `uuid`, returning it if present.
    ///
    /// Clones already handed out keep the old asset alive until they drop, so
    /// a reader in flight never sees a half-loaded replacement. A removal is
    /// recorded as [`ArrivalKind::Gone`]; removing nothing records nothing.
    pub fn remove(&mut self, uuid: &AssetUUID) -> Option<AssetHandle<A>> {
        let removed = self.storage.remove(uuid);
        if removed.is_some() {
            self.journal.lock_recovered().record(Arrival {
                uuid: *uuid,
                kind: ArrivalKind::Gone,
            });
        }
        removed
    }

    /// Returns an iterator over every cached UUID.
    pub fn keys(&self) -> impl Iterator<Item = AssetUUID> + '_ {
        self.storage.keys().copied()
    }

    /// Returns the number of cached assets.
    pub fn len(&self) -> usize {
        self.storage.len()
    }

    /// Returns `true` if no assets are cached.
    pub fn is_empty(&self) -> bool {
        self.storage.is_empty()
    }

    /// Reads the arrivals `reader` has not seen yet and moves its cursor to the latest one.
    ///
    /// A reader that is new, or some of whose unread arrivals were dropped
    /// past the journal's cap, gets [`ArrivalsRead::Resync`] and must read the whole store.
    pub fn arrivals(&self, reader: &'static str) -> ArrivalsRead {
        self.journal.lock_recovered().read(reader)
    }

    /// Returns the number of entries the arrivals journal holds.
    #[cfg(test)]
    fn arrivals_held(&self) -> usize {
        self.journal.lock_recovered().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Arrival, ArrivalKind, SoundData, ARRIVALS_KEPT};

    /// Local newtype wrapper so the orphan rule lets us `impl Asset` for it.
    /// `Asset` is just a marker trait (`Send + Sync + 'static`).
    #[derive(Debug, PartialEq)]
    struct TestAsset(String);
    impl Asset for TestAsset {}

    const RESIDENCY: &str = "residency";
    const TEXT_LAYOUT: &str = "text_layout";

    fn copy(name: &str) -> AssetHandle<TestAsset> {
        AssetHandle::new(TestAsset(name.into()))
    }

    fn uuid(path: &str) -> AssetUUID {
        AssetUUID::new_v5(path)
    }

    fn landed(uuid: AssetUUID) -> Arrival {
        Arrival {
            uuid,
            kind: ArrivalKind::Landed,
        }
    }

    fn replaced(uuid: AssetUUID) -> Arrival {
        Arrival {
            uuid,
            kind: ArrivalKind::Replaced,
        }
    }

    fn gone(uuid: AssetUUID) -> Arrival {
        Arrival {
            uuid,
            kind: ArrivalKind::Gone,
        }
    }

    /// Returns a store of `kept` journal entries on which `readers` have each read once.
    fn store_read_by(kept: usize, readers: &[&'static str]) -> Assets<TestAsset> {
        let assets = Assets::<TestAsset>::with_arrivals_kept(kept);
        for reader in readers {
            assert_eq!(assets.arrivals(reader), ArrivalsRead::Resync);
        }
        assets
    }

    /// Asserts `assets` journal keeps [`ARRIVALS_KEPT`] unread entries for a reader, and no more.
    fn assert_keeps_arrivals_kept<A: Asset>(
        mut assets: Assets<A>,
        make: impl Fn() -> AssetHandle<A>,
    ) {
        assert_eq!(assets.arrivals(RESIDENCY), ArrivalsRead::Resync);
        for i in 0..ARRIVALS_KEPT {
            assets.insert(uuid(&format!("mesh-{i}")), make());
        }
        assert_eq!(assets.arrivals_held(), ARRIVALS_KEPT);
        match assets.arrivals(RESIDENCY) {
            ArrivalsRead::Since(read) => assert_eq!(read.len(), ARRIVALS_KEPT),
            ArrivalsRead::Resync => panic!("a reader {ARRIVALS_KEPT} behind must not resync"),
        }

        for i in 0..=ARRIVALS_KEPT {
            assets.insert(uuid(&format!("mesh-{i}")), make());
        }
        // Its only reader fell past the cap: no reader can read what is left.
        assert_eq!(assets.arrivals_held(), 0);
        assert_eq!(assets.arrivals(RESIDENCY), ArrivalsRead::Resync);
    }

    #[test]
    fn a_readers_first_read_is_a_resync() {
        let mut assets = Assets::<TestAsset>::new();
        assert_eq!(assets.arrivals(RESIDENCY), ArrivalsRead::Resync);

        // A reader new to a store that already holds arrivals resyncs too.
        assets.insert(uuid("a"), copy("a"));
        assert_eq!(assets.arrivals(TEXT_LAYOUT), ArrivalsRead::Resync);
    }

    #[test]
    fn a_resync_leaves_the_cursor_at_the_latest_arrival() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));
        assert_eq!(assets.arrivals(TEXT_LAYOUT), ArrivalsRead::Resync);

        assert_eq!(assets.arrivals(TEXT_LAYOUT), ArrivalsRead::Since(vec![]));
    }

    #[test]
    fn a_copy_inserted_after_a_read_is_landed_on_the_next_read() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));

        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );
    }

    #[test]
    fn inserting_under_a_uuid_already_held_is_replaced() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("first"));
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );

        assets.insert(uuid("a"), copy("second"));
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![replaced(uuid("a"))])
        );
    }

    #[test]
    fn removing_a_held_copy_is_gone_and_removing_an_absent_one_records_nothing() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );

        assert!(assets.remove(&uuid("a")).is_some());
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![gone(uuid("a"))])
        );

        assert!(assets.remove(&uuid("a")).is_none());
        assert!(assets.remove(&uuid("never-held")).is_none());
        assert_eq!(assets.arrivals(RESIDENCY), ArrivalsRead::Since(vec![]));
    }

    #[test]
    fn a_copy_removed_then_inserted_again_is_landed() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("first"));
        assets.remove(&uuid("a"));
        assets.insert(uuid("a"), copy("second"));

        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a")), gone(uuid("a")), landed(uuid("a"))])
        );
    }

    #[test]
    fn each_reader_sees_each_arrival_once() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );

        assert_eq!(assets.arrivals(RESIDENCY), ArrivalsRead::Since(vec![]));
    }

    #[test]
    fn two_readers_read_independently() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY, TEXT_LAYOUT]);
        assets.insert(uuid("a"), copy("a"));
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );

        assets.insert(uuid("b"), copy("b"));
        assert_eq!(
            assets.arrivals(TEXT_LAYOUT),
            ArrivalsRead::Since(vec![landed(uuid("a")), landed(uuid("b"))])
        );
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("b"))])
        );
    }

    #[test]
    fn arrivals_come_in_the_order_they_happened() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));
        assets.insert(uuid("b"), copy("b"));
        assets.remove(&uuid("a"));
        assets.insert(uuid("b"), copy("b2"));
        assets.insert(uuid("c"), copy("c"));

        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![
                landed(uuid("a")),
                landed(uuid("b")),
                gone(uuid("a")),
                replaced(uuid("b")),
                landed(uuid("c")),
            ])
        );
    }

    #[test]
    fn entries_every_reader_has_read_are_dropped() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY, TEXT_LAYOUT]);
        assets.insert(uuid("a"), copy("a"));
        assets.insert(uuid("b"), copy("b"));
        assert_eq!(assets.arrivals_held(), 2);

        // One reader has read them: the other still needs them.
        assets.arrivals(RESIDENCY);
        assert_eq!(assets.arrivals_held(), 2);

        assets.insert(uuid("c"), copy("c"));
        assets.arrivals(TEXT_LAYOUT);
        // `a` and `b` are read by both; `c` is not read by the residency yet.
        assert_eq!(assets.arrivals_held(), 1);

        assets.arrivals(RESIDENCY);
        assert_eq!(assets.arrivals_held(), 0);
    }

    #[test]
    fn a_reader_past_the_cap_reads_resync_and_the_journal_stays_under_it() {
        const KEPT: usize = 3;
        let mut assets = store_read_by(KEPT, &[RESIDENCY, TEXT_LAYOUT]);
        for i in 0..5 {
            let path = format!("mesh-{i}");
            assets.insert(uuid(&path), copy(&path));
            assert!(assets.arrivals_held() <= KEPT);
            // The residency keeps up; the text layout does not read.
            assert_eq!(
                assets.arrivals(RESIDENCY),
                ArrivalsRead::Since(vec![landed(uuid(&path))])
            );
        }
        // The text layout fell behind and the residency has read everything.
        assert_eq!(assets.arrivals_held(), 0);

        assert_eq!(assets.arrivals(TEXT_LAYOUT), ArrivalsRead::Resync);
        assert_eq!(assets.arrivals_held(), 0);

        // After the resync its cursor stands at the latest arrival.
        assets.insert(uuid("after"), copy("after"));
        assert_eq!(
            assets.arrivals(TEXT_LAYOUT),
            ArrivalsRead::Since(vec![landed(uuid("after"))])
        );
    }

    #[test]
    fn a_reader_exactly_the_cap_behind_reads_every_arrival() {
        const KEPT: usize = 3;
        let mut assets = store_read_by(KEPT, &[RESIDENCY]);
        let uuids: Vec<AssetUUID> = (0..KEPT).map(|i| uuid(&format!("mesh-{i}"))).collect();
        for (i, id) in uuids.iter().enumerate() {
            assets.insert(*id, copy(&format!("mesh-{i}")));
        }

        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(uuids.into_iter().map(landed).collect())
        );
    }

    #[test]
    fn a_new_store_keeps_arrivals_kept_entries() {
        assert_keeps_arrivals_kept(Assets::<TestAsset>::new(), || copy("mesh"));
    }

    #[test]
    fn a_default_store_keeps_arrivals_kept_entries() {
        assert_keeps_arrivals_kept(Assets::<SoundData>::default(), || {
            AssetHandle::new(SoundData::default())
        });
    }

    #[test]
    fn a_clones_journal_starts_empty_and_without_readers() {
        let mut assets = store_read_by(ARRIVALS_KEPT, &[RESIDENCY]);
        assets.insert(uuid("a"), copy("a"));

        let clone = assets.clone();
        assert!(clone.contains(&uuid("a")));
        assert_eq!(clone.arrivals_held(), 0);
        assert_eq!(clone.arrivals(RESIDENCY), ArrivalsRead::Resync);

        // The original's reader still has its arrival.
        assert_eq!(
            assets.arrivals(RESIDENCY),
            ArrivalsRead::Since(vec![landed(uuid("a"))])
        );
    }

    #[test]
    fn arrivals_recorded_while_no_reader_reads_are_not_held() {
        // A reader that comes later resyncs, so no one can ever read them.
        let mut assets = Assets::<TestAsset>::new();
        assets.insert(uuid("a"), copy("a"));
        assets.insert(uuid("a"), copy("a2"));
        assets.remove(&uuid("a"));

        assert_eq!(assets.arrivals_held(), 0);
    }

    #[test]
    fn a_reader_that_must_resync_does_not_hold_what_the_others_read() {
        const KEPT: usize = 3;
        let mut assets = store_read_by(KEPT, &[RESIDENCY, TEXT_LAYOUT]);
        // The residency reads every arrival; the text layout stops reading and
        // falls past the cap, so it resyncs whatever is held.
        for i in 0..=KEPT {
            let path = format!("mesh-{i}");
            assets.insert(uuid(&path), copy(&path));
            assert!(matches!(assets.arrivals(RESIDENCY), ArrivalsRead::Since(_)));
        }

        assert_eq!(assets.arrivals_held(), 0);
        assert_eq!(assets.arrivals(TEXT_LAYOUT), ArrivalsRead::Resync);
    }

    #[test]
    fn remove_drops_cached_handle() {
        let mut assets = Assets::<TestAsset>::new();
        let uuid = AssetUUID::new_v5("textures/foo.png");
        assets.insert(uuid, AssetHandle::new(TestAsset("hello".into())));

        assert!(assets.contains(&uuid));
        let popped = assets.remove(&uuid);
        assert!(popped.is_some());
        assert!(!assets.contains(&uuid));
        // Removing again returns None (idempotent).
        assert!(assets.remove(&uuid).is_none());
    }
}
